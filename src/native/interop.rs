//! Typed unsafe boundaries for host-created native devices and images.
#[cfg(all(target_os = "windows", feature = "dx12"))]
pub mod dx12;
#[cfg(all(target_os = "windows", feature = "vulkan"))]
pub mod vulkan;

#[derive(Clone)]
pub(crate) enum Synchronization {
    #[cfg(all(target_os = "windows", feature = "dx12"))]
    Dx12(dx12::TargetSynchronization),
    #[cfg(all(target_os = "windows", feature = "vulkan"))]
    Vulkan(vulkan::TargetSynchronization),
}
impl Synchronization {
    pub(crate) fn outgoing(&self) -> crate::NativeTargetState {
        match *self {
            #[cfg(all(target_os = "windows", feature = "dx12"))]
            Self::Dx12(ref sync) => crate::NativeTargetState {
                state: sync.outgoing,
            },
            #[cfg(all(target_os = "windows", feature = "vulkan"))]
            Self::Vulkan(ref sync) => crate::NativeTargetState {
                state: sync.outgoing,
            },
        }
    }
}
