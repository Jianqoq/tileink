//! Shared scheduler boundary. Native commands own input bytes; submission copies
//! staged uniforms before CommandBatch reuses an arena. API owners retire leases.
#[cfg(feature = "metal")]
mod metal;
#[cfg(test)]
use super::program::{Dispatch, Params};
use super::{Result, submissions::Ticket};
#[cfg(test)]
use crate::render::{backend::BatchAdapter, upload::uniforms::UniformWrites};
use crate::{native::NativeBackend, render::backend::SubmitError};
#[cfg(test)]
use std::hash::{Hash, Hasher};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub struct Adapter(Rc<RefCell<Device>>);

#[derive(Clone)]
pub struct Receipt {
    owner: Adapter,
    ticket: Ticket,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.ticket.fmt(f)
    }
}
impl Receipt {
    pub fn is_complete(&self) -> Result<bool> {
        match &mut *self.owner.0.borrow_mut() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.is_complete(&self.ticket),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.is_complete(&self.ticket),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.is_complete(&self.ticket),
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        self.owner.readback(self)
    }
    pub fn assert_valid(&self) -> Result<()> {
        self.owner.assert_valid()
    }
}

enum Device {
    #[cfg(feature = "metal")]
    Metal(Box<super::metal::Metal>),
    #[cfg(feature = "dx12")]
    Dx12(Box<super::dx12::Dx12>),
    #[cfg(feature = "vulkan")]
    Vulkan(Box<super::vulkan::Vulkan>),
}

#[cfg(test)]
// Allocation identity and device generation are separate. Rc<()> value equality
// would equate all allocations, so only pointer identity participates in Eq/Hash.
#[derive(Clone)]
pub struct UniformBuffer {
    device: Rc<RefCell<Device>>,
    allocation: Rc<()>,
}
#[cfg(test)]
impl PartialEq for UniformBuffer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.allocation, &other.allocation)
    }
}
#[cfg(test)]
impl Eq for UniformBuffer {}
#[cfg(test)]
impl Hash for UniformBuffer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.allocation).hash(state);
    }
}

#[cfg(test)]
pub struct Encoder {
    device: Rc<RefCell<Device>>,
    commands: Vec<Command>,
}
#[cfg(test)]
struct Command {
    dispatch: Dispatch,
    uniform: Option<(UniformBuffer, u64)>,
}
#[cfg(test)]
impl Encoder {
    pub fn dispatch(&mut self, dispatch: impl Into<Dispatch>) {
        self.commands.push(Command {
            dispatch: dispatch.into(),
            uniform: None,
        });
    }
    pub fn dispatch_uniform(
        &mut self,
        dispatch: impl Into<Dispatch>,
        buffer: UniformBuffer,
        offset: u64,
    ) {
        self.commands.push(Command {
            dispatch: dispatch.into(),
            uniform: Some((buffer, offset)),
        });
    }
}

impl Adapter {
    #[cfg(feature = "dx12")]
    pub fn import_dx12_texture(
        &self,
        descriptor: crate::native::interop::dx12::TextureDescriptor,
    ) -> Result<(super::texture::Allocation, [u32; 2])> {
        match &*self.0.borrow() {
            Device::Dx12(device) => device.import_texture(descriptor),
            #[cfg(feature = "vulkan")]
            _ => Err("DX12 image requires a DX12 context".into()),
        }
    }

    #[cfg(feature = "vulkan")]
    pub fn from_vulkan(
        descriptor: crate::native::interop::vulkan::ContextDescriptor,
    ) -> Result<Self> {
        Ok(Self(Rc::new(RefCell::new(Device::Vulkan(Box::new(
            super::vulkan::Vulkan::from_imported(descriptor)?,
        ))))))
    }

    #[cfg(feature = "dx12")]
    pub fn from_dx12(descriptor: crate::native::interop::dx12::ContextDescriptor) -> Result<Self> {
        Ok(Self(Rc::new(RefCell::new(Device::Dx12(Box::new(
            super::dx12::Dx12::from_imported(descriptor)?,
        ))))))
    }

    pub fn same_device(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn allocate_buffer(&self, size: usize) -> Result<super::buffer::Allocation> {
        match &*self.0.borrow() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.allocate_buffer(size),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.allocate_buffer(size),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.allocate_buffer(size),
        }
    }
    #[cfg(feature = "vulkan")]
    pub fn import_vulkan_texture(
        &self,
        descriptor: crate::native::interop::vulkan::TextureDescriptor,
    ) -> Result<super::texture::Allocation> {
        match &*self.0.borrow() {
            Device::Vulkan(device) => device.import_texture(descriptor),
            #[cfg(feature = "dx12")]
            _ => Err("Vulkan image requires a Vulkan context".into()),
        }
    }
    pub fn allocate_texture(
        &self,
        size: [u32; 2],
        layers: u32,
        array: bool,
    ) -> Result<super::texture::Allocation> {
        match &*self.0.borrow() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.allocate_texture(size, layers, array),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.allocate_texture(size, layers, array),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.allocate_texture(size, layers, array),
        }
    }
    #[cfg(test)]
    pub fn new(backend: NativeBackend, identity: &str) -> Result<Self> {
        #[cfg(feature = "dx12")]
        if backend == NativeBackend::Dx12 {
            // Verification enters native construction before its wgpu references.
            unsafe {
                super::enable_dx12_validation()?;
            }
        }
        Self::with_options(
            backend,
            &crate::native::NativeContextOptions {
                physical_adapter: Some(identity.to_owned()),
                validation: true,
            },
        )
    }
    pub fn with_options(
        backend: NativeBackend,
        options: &crate::native::NativeContextOptions,
    ) -> Result<Self> {
        let device = match backend {
            #[cfg(feature = "metal")]
            NativeBackend::Metal => {
                Device::Metal(Box::new(super::metal::Metal::with_options(options)?))
            }
            #[cfg(feature = "dx12")]
            NativeBackend::Dx12 => {
                Device::Dx12(Box::new(super::dx12::Dx12::with_options(options)?))
            }
            #[cfg(feature = "vulkan")]
            NativeBackend::Vulkan => {
                Device::Vulkan(Box::new(super::vulkan::Vulkan::with_options(options)?))
            }
            #[cfg(not(all(feature = "dx12", feature = "vulkan")))]
            _ => return Err("native API feature is disabled".into()),
        };
        Ok(Self(Rc::new(RefCell::new(device))))
    }
    pub fn limits(&self) -> super::renderer::recording::Limits {
        match &*self.0.borrow() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.limits(),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.limits(),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.limits(),
        }
    }
    pub fn submit_compute(
        &self,
        batch: &super::compute::ComputeBatch,
    ) -> std::result::Result<Receipt, SubmitError<Box<dyn std::error::Error>>> {
        for resource in batch.resources() {
            if let super::compute::Resource::PersistentBuffer(upload) = resource
                && !self.same_device(&upload.buffer.adapter)
            {
                return Err(SubmitError::Rejected(
                    "persistent buffer belongs to another logical device".into(),
                ));
            }
            if let super::compute::Resource::Texture(texture) = resource
                && let Some(texture) = &texture.persistent
                && !self.same_device(&texture.context.adapter)
            {
                return Err(SubmitError::Rejected(
                    "persistent texture belongs to another logical device".into(),
                ));
            }
        }
        let mut device = self.0.borrow_mut();
        let (result, unconfirmed) = match &mut *device {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => (device.submit_compute(batch), device.unconfirmed()),
            #[cfg(feature = "metal")]
            Device::Metal(device) => (device.submit_compute(batch), device.unconfirmed()),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => (device.submit_compute(batch), device.unconfirmed()),
        };
        result
            .map(|ticket| {
                batch.confirm_submission();
                Receipt {
                    owner: self.clone(),
                    ticket,
                }
            })
            .map_err(|error| {
                if unconfirmed {
                    SubmitError::Unconfirmed(error)
                } else {
                    SubmitError::Rejected(error)
                }
            })
    }
    #[cfg(test)]
    pub fn uniform_buffer(&self) -> UniformBuffer {
        UniformBuffer {
            device: self.0.clone(),
            allocation: Rc::new(()),
        }
    }
    pub fn readback(&self, receipt: &Receipt) -> Result<Vec<Vec<u8>>> {
        if !Rc::ptr_eq(&self.0, &receipt.owner.0) {
            return Err("receipt belongs to another device generation".into());
        }
        let ticket = &receipt.ticket;
        match &mut *self.0.borrow_mut() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.readback_batch(ticket),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.readback_batch(ticket),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.readback_batch(ticket),
        }
    }
    pub fn pending_count(&self) -> usize {
        match &*self.0.borrow() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.pending_count(),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.pending_count(),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.pending_count(),
        }
    }
    pub fn assert_valid(&self) -> Result<()> {
        match &*self.0.borrow() {
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.assert_valid(),
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => super::dx12::assert_valid(&device.validation_queue()),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => {
                let messages = device.validation_messages();
                let messages = messages.lock().unwrap_or_else(|e| e.into_inner());
                if messages.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Vulkan validation: {messages:?}").into())
                }
            }
        }
    }
    #[cfg(test)]
    fn resolve(
        &self,
        encoder: Encoder,
        uniforms: &UniformWrites<UniformBuffer>,
    ) -> Result<Vec<Dispatch>> {
        if !Rc::ptr_eq(&self.0, &encoder.device) {
            return Err("encoder belongs to another device".into());
        }
        for (buffer, _) in uniforms.iter() {
            if !Rc::ptr_eq(&self.0, &buffer.device) {
                return Err("uniform belongs to another device".into());
            }
        }
        encoder
            .commands
            .into_iter()
            .map(|mut command| {
                if let Some((buffer, offset)) = command.uniform {
                    if !Rc::ptr_eq(&self.0, &buffer.device) {
                        return Err("uniform belongs to another device".into());
                    }
                    if !offset.is_multiple_of(256) {
                        return Err("unaligned native uniform slot".into());
                    }
                    let start = usize::try_from(offset)?;
                    let end = start
                        .checked_add(std::mem::size_of::<Params>())
                        .ok_or("uniform range overflow")?;
                    let data = uniforms
                        .iter()
                        .find(|(key, _)| **key == buffer)
                        .ok_or("uniform not staged")?
                        .1;
                    let Dispatch::Probe(probe) = &mut command.dispatch else {
                        return Err("program has no uniform block".into());
                    };
                    probe.params = bytemuck::pod_read_unaligned(
                        data.get(start..end).ok_or("uniform range out of bounds")?,
                    );
                }
                Ok(command.dispatch)
            })
            .collect()
    }
}

#[cfg(test)]
impl BatchAdapter for Adapter {
    type Buffer = UniformBuffer;
    type Encoder = Encoder;
    type Submission = Receipt;
    type Error = Box<dyn std::error::Error>;
    fn create_encoder(&mut self, _label: &'static str) -> Result<Encoder> {
        Ok(Encoder {
            device: self.0.clone(),
            commands: Vec::new(),
        })
    }
    fn submit(
        &mut self,
        encoder: Encoder,
        uniforms: &UniformWrites<UniformBuffer>,
    ) -> std::result::Result<Receipt, SubmitError<Self::Error>> {
        let commands = self
            .resolve(encoder, uniforms)
            .map_err(SubmitError::Rejected)?;
        let mut device = self.0.borrow_mut();
        let (result, unconfirmed) = match &mut *device {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => (device.submit_batch(&commands), device.unconfirmed()),
            #[cfg(feature = "metal")]
            Device::Metal(device) => (device.submit_batch(&commands), device.unconfirmed()),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => (device.submit_batch(&commands), device.unconfirmed()),
        };
        result
            .map(|ticket| Receipt {
                owner: self.clone(),
                ticket,
            })
            .map_err(|error| {
                if unconfirmed {
                    SubmitError::Unconfirmed(error)
                } else {
                    SubmitError::Rejected(error)
                }
            })
    }
}

impl Adapter {
    pub fn assert_valid_with_wgpu_clears(&self) -> Result<()> {
        #[cfg(feature = "dx12")]
        {
            let Device::Dx12(device) = &*self.0.borrow();
            super::dx12::assert_valid_with_wgpu_clears(&device.validation_queue())
        }
        #[cfg(any(feature = "vulkan", feature = "metal"))]
        self.assert_valid()
    }
}

#[cfg(test)]
impl Adapter {
    pub(crate) fn import_context_for_test(
        host: &crate::NativeContext,
    ) -> Result<crate::NativeContext> {
        unsafe {
            match &*host.adapter.0.borrow() {
                #[cfg(feature = "metal")]
                Device::Metal(device) => Ok(crate::NativeContext::from_metal(
                    crate::native::interop::metal::ContextDescriptor {
                        device: device.device.clone(),
                        queue: device.queue.clone(),
                    },
                )?),
                #[cfg(feature = "dx12")]
                Device::Dx12(device) => {
                    Ok(crate::NativeContext::from_dx12(device.import_descriptor())?)
                }
                #[cfg(feature = "vulkan")]
                Device::Vulkan(device) => Ok(crate::NativeContext::from_vulkan(
                    device.import_descriptor(Rc::new(host.clone())),
                )?),
            }
        }
    }
}

#[cfg(all(test, feature = "dx12"))]
impl Adapter {
    pub(crate) fn dx12_device_for_test(
        &self,
    ) -> windows::Win32::Graphics::Direct3D12::ID3D12Device {
        match &*self.0.borrow() {
            Device::Dx12(device) => device.import_descriptor().device,
        }
    }
}

#[cfg(all(test, feature = "vulkan"))]
impl Adapter {
    pub(crate) fn vulkan_descriptor_for_test(
        host: &crate::NativeContext,
    ) -> crate::native::interop::vulkan::ContextDescriptor {
        match &*host.adapter.0.borrow() {
            Device::Vulkan(device) => device.import_descriptor(Rc::new(host.clone())),
        }
    }
}

#[cfg(all(test, feature = "dx12"))]
impl Adapter {
    pub(crate) fn inject_dx12_signal_failure_for_test(&self) {
        match &mut *self.0.borrow_mut() {
            Device::Dx12(device) => device.inject_signal_failure_for_test(),
        }
    }
}
