//! Typed unsafe boundaries for host-created native devices and images.
#[cfg(all(target_os = "windows", feature = "dx12"))]
pub mod dx12;
#[cfg(all(target_os = "macos", feature = "metal"))]
pub mod metal;
#[cfg(all(target_os = "windows", feature = "vulkan"))]
pub mod vulkan;

#[derive(Clone)]
pub(crate) enum Synchronization {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    Metal(metal::TargetSynchronization),
    #[cfg(all(target_os = "windows", feature = "dx12"))]
    Dx12(dx12::TargetSynchronization),
    #[cfg(all(target_os = "windows", feature = "vulkan"))]
    Vulkan(vulkan::TargetSynchronization),
}
impl Synchronization {
    #[cfg_attr(
        not(tileink_native_runtime),
        allow(
            clippy::uninhabited_references,
            reason = "Without a native adapter this enum has no variants and safe code cannot construct a synchronization contract"
        )
    )]
    pub(crate) const fn outgoing(&self) -> crate::NativeTargetState {
        #[cfg(not(tileink_native_runtime))]
        match *self {}
        #[cfg(tileink_native_runtime)]
        match self {
            #[cfg(all(target_os = "macos", feature = "metal"))]
            Self::Metal(_) => crate::NativeTargetState {},
            #[cfg(all(target_os = "windows", feature = "dx12"))]
            Self::Dx12(sync) => crate::NativeTargetState {
                state: sync.outgoing,
            },
            #[cfg(all(target_os = "windows", feature = "vulkan"))]
            Self::Vulkan(sync) => crate::NativeTargetState {
                state: sync.outgoing,
            },
        }
    }
}
