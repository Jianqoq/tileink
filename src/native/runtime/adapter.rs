//! Native device ownership and submission receipts for recorded compute batches.
#[cfg(feature = "metal")]
mod metal;
use super::{Result, submissions::Ticket};
use crate::native::NativeBackend;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug)]
pub(crate) enum SubmitError {
    /// The attempted submission was not accepted; earlier batches may still run.
    Rejected(Box<dyn std::error::Error>),
    /// Work may have been accepted without a usable completion receipt. The
    /// adapter keeps its leases and reports recovery/device loss to the caller.
    Unconfirmed(Box<dyn std::error::Error>),
}

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
        format: crate::ShaderTextureFormat,
    ) -> Result<super::texture::Allocation> {
        match &*self.0.borrow() {
            #[cfg(feature = "dx12")]
            Device::Dx12(device) => device.allocate_texture(size, layers, array, format),
            #[cfg(feature = "metal")]
            Device::Metal(device) => device.allocate_texture(size, layers, array, format),
            #[cfg(feature = "vulkan")]
            Device::Vulkan(device) => device.allocate_texture(size, layers, array, format),
        }
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
    ) -> std::result::Result<Receipt, SubmitError> {
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
}

#[cfg(all(test, any(feature = "dx12", feature = "vulkan")))]
impl Adapter {
    pub(crate) fn import_context_for_test(
        host: &crate::NativeContext,
    ) -> Result<crate::NativeContext> {
        unsafe {
            match &*host.adapter.0.borrow() {
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
