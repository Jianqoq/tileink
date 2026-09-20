//! Host-created Metal objects retain Objective-C ownership. Queue order or shared
//! event waits/signals establish access ownership across host and renderer work.
use crate::native::{
    NativeBackend, NativeContext, NativeError, NativeRenderTarget, NativeTargetUse, NativeTexture,
};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::*;

pub struct ContextDescriptor {
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}
pub struct TextureDescriptor {
    pub texture: Retained<ProtocolObject<dyn MTLTexture>>,
    pub initialized: bool,
}
#[derive(Clone)]
pub struct EventPoint {
    pub event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    pub value: u64,
}
#[derive(Clone, Default)]
pub struct TargetSynchronization {
    pub waits: Vec<EventPoint>,
    pub signals: Vec<EventPoint>,
}
impl TargetSynchronization {
    pub(crate) fn validate(
        &self,
        device: &ProtocolObject<dyn MTLDevice>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for point in self.waits.iter().chain(&self.signals) {
            if point
                .event
                .device()
                .is_some_and(|owner| !std::ptr::eq(&*owner, device))
            {
                return Err("Metal event belongs to another device".into());
            }
        }
        for (index, signal) in self.signals.iter().enumerate() {
            if self.signals[..index]
                .iter()
                .any(|old| std::ptr::eq(&*old.event, &*signal.event))
            {
                return Err("duplicate Metal event signal".into());
            }
            if self.waits.iter().any(|wait| {
                std::ptr::eq(&*wait.event, &*signal.event) && wait.value >= signal.value
            }) {
                return Err("Metal event signal must advance the waited value".into());
            }
        }
        Ok(())
    }
}
impl NativeContext {
    /// # Safety
    /// The host must serialize access to this queue and imported resources or
    /// supply event synchronization. No outstanding host encoder may race with
    /// Tileink. Retaining an object does not authorize concurrent pixel access.
    pub unsafe fn from_metal(descriptor: ContextDescriptor) -> Result<Self, NativeError> {
        let adapter = super::super::runtime::adapter::Adapter::from_metal(descriptor)
            .map_err(NativeError::Initialization)?;
        Ok(Self::from_adapter(NativeBackend::Metal, adapter))
    }
    /// # Safety
    /// The host guarantees exclusive access until the returned submission is
    /// complete, or uses a per-use event contract. `initialized` asserts every
    /// texel is defined. External writes invalidate any retained target history.
    pub unsafe fn import_metal_texture(
        &self,
        descriptor: TextureDescriptor,
    ) -> Result<NativeTexture, NativeError> {
        self.adapter
            .import_metal_texture(self, descriptor)
            .map_err(NativeError::Initialization)
    }
}
impl<'a> NativeRenderTarget<'a> {
    /// # Safety
    /// Wait values must be signaled by the host; no host access may overlap the
    /// interval between those waits and the outgoing signals/completion token.
    pub unsafe fn with_metal_synchronization(
        self,
        synchronization: TargetSynchronization,
    ) -> NativeTargetUse<'a> {
        NativeTargetUse {
            target: self,
            synchronization: super::Synchronization::Metal(synchronization),
        }
    }
}
