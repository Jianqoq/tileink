//! Native execution modules for the M3 minimum vertical slice.
//! Full Canvas program coverage and the NativeRenderer facade follow in M4.
pub(super) mod adapter;
#[cfg(feature = "native-dx12")]
mod dx12;
#[cfg(all(
    test,
    feature = "wgpu",
    feature = "native-dx12",
    feature = "native-vulkan"
))]
mod gpu_tests;
#[cfg(test)]
#[path = "runtime/tests/isolation.rs"]
mod isolation;
mod pipeline_cache;
mod program;
mod submissions;
#[cfg(feature = "native-vulkan")]
mod vulkan;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(feature = "native-dx12")]
pub(super) unsafe fn enable_dx12_validation() -> Result<()> {
    unsafe { dx12::enable_validation() }
}

#[path = "runtime/compute.rs"]
pub(super) mod compute;
pub(super) mod renderer;
