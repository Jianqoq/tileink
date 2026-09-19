//! Typed unsafe boundaries for host-created native devices and images.
#[cfg(all(target_os = "windows", feature = "native-dx12"))]
pub mod dx12;
#[cfg(all(target_os = "windows", feature = "native-vulkan"))]
pub mod vulkan;

#[derive(Clone)]
pub(crate) enum Synchronization {
    #[cfg(all(target_os = "windows", feature = "native-dx12"))]
    Dx12(dx12::TargetSynchronization),
    #[cfg(all(target_os = "windows", feature = "native-vulkan"))]
    Vulkan(vulkan::TargetSynchronization),
}
impl Synchronization {
    pub(crate) fn outgoing(&self) -> crate::NativeTargetState {
        match *self {
            #[cfg(all(target_os = "windows", feature = "native-dx12"))]
            Self::Dx12(ref sync) => crate::NativeTargetState::Dx12(sync.outgoing),
            #[cfg(all(target_os = "windows", feature = "native-vulkan"))]
            Self::Vulkan(ref sync) => crate::NativeTargetState::Vulkan(sync.outgoing),
        }
    }
}
