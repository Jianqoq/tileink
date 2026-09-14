//! Shared scheduler boundary. Native commands own input bytes; submission copies
//! staged uniforms before CommandBatch reuses an arena. API owners retire leases.
use super::{
    Result,
    program::{Dispatch, Params},
    submissions::Ticket,
};
use crate::{
    native::NativeBackend,
    render::{
        backend::{BatchAdapter, SubmitError},
        upload::uniforms::UniformWrites,
    },
};
use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
    rc::Rc,
};

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
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        self.owner.readback(self)
    }
    pub fn assert_valid(&self) -> Result<()> {
        self.owner.assert_valid()
    }
}

enum Device {
    #[cfg(feature = "native-dx12")]
    Dx12(Box<super::dx12::Dx12>),
    #[cfg(feature = "native-vulkan")]
    Vulkan(Box<super::vulkan::Vulkan>),
}

// Allocation identity and device generation are separate. Rc<()> value equality
// would equate all allocations, so only pointer identity participates in Eq/Hash.
#[derive(Clone)]
pub struct UniformBuffer {
    device: Rc<RefCell<Device>>,
    allocation: Rc<()>,
}
impl PartialEq for UniformBuffer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.allocation, &other.allocation)
    }
}
impl Eq for UniformBuffer {}
impl Hash for UniformBuffer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.allocation).hash(state);
    }
}

pub struct Encoder {
    device: Rc<RefCell<Device>>,
    commands: Vec<Command>,
}
struct Command {
    dispatch: Dispatch,
    uniform: Option<(UniformBuffer, u64)>,
}
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
    pub fn new(backend: NativeBackend, identity: &str) -> Result<Self> {
        let device = match backend {
            #[cfg(feature = "native-dx12")]
            NativeBackend::Dx12 => Device::Dx12(Box::new(super::dx12::Dx12::new(identity)?)),
            #[cfg(feature = "native-vulkan")]
            NativeBackend::Vulkan => {
                Device::Vulkan(Box::new(super::vulkan::Vulkan::new(identity)?))
            }
            #[cfg(not(all(feature = "native-dx12", feature = "native-vulkan")))]
            _ => return Err("native API feature is disabled".into()),
        };
        Ok(Self(Rc::new(RefCell::new(device))))
    }
    pub fn submit_compute(
        &self,
        batch: &super::compute::ComputeBatch,
    ) -> std::result::Result<Receipt, SubmitError<Box<dyn std::error::Error>>> {
        let mut device = self.0.borrow_mut();
        let (result, unconfirmed) = match &mut *device {
            #[cfg(feature = "native-dx12")]
            Device::Dx12(device) => (device.submit_compute(batch), device.unconfirmed()),
            #[cfg(feature = "native-vulkan")]
            Device::Vulkan(device) => (device.submit_compute(batch), device.unconfirmed()),
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
            #[cfg(feature = "native-dx12")]
            Device::Dx12(device) => device.readback_batch(ticket),
            #[cfg(feature = "native-vulkan")]
            Device::Vulkan(device) => device.readback_batch(ticket),
        }
    }
    pub fn pending_count(&self) -> usize {
        match &*self.0.borrow() {
            #[cfg(feature = "native-dx12")]
            Device::Dx12(device) => device.pending_count(),
            #[cfg(feature = "native-vulkan")]
            Device::Vulkan(device) => device.pending_count(),
        }
    }
    pub fn assert_valid(&self) -> Result<()> {
        match &*self.0.borrow() {
            #[cfg(feature = "native-dx12")]
            Device::Dx12(device) => super::dx12::assert_valid(&device.validation_queue()),
            #[cfg(feature = "native-vulkan")]
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
            #[cfg(feature = "native-dx12")]
            Device::Dx12(device) => (device.submit_batch(&commands), device.unconfirmed()),
            #[cfg(feature = "native-vulkan")]
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

#[cfg(test)]
impl Adapter {
    pub fn assert_valid_with_wgpu_clears(&self) -> Result<()> {
        #[cfg(feature = "native-dx12")]
        if let Device::Dx12(device) = &*self.0.borrow() {
            return super::dx12::assert_valid_with_wgpu_clears(&device.validation_queue());
        }
        self.assert_valid()
    }
}
