//! Native execution modules for the M3 minimum vertical slice.
//! Full Canvas program coverage and the NativeRenderer facade follow in M4.
pub(super) mod adapter;
#[cfg(feature = "dx12")]
mod dx12;
#[cfg(all(test, feature = "wgpu", feature = "dx12", feature = "vulkan"))]
mod gpu_tests;
#[cfg(test)]
#[path = "runtime/tests/isolation.rs"]
mod isolation;
#[cfg(test)]
mod lifecycle_gpu_tests;
mod pipeline_cache;
mod program;
mod submissions;
pub(super) mod texture;
#[cfg(feature = "vulkan")]
mod vulkan;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(feature = "dx12")]
pub(super) unsafe fn enable_dx12_validation() -> Result<()> {
    unsafe { dx12::enable_validation() }
}

#[path = "runtime/compute.rs"]
pub(super) mod compute;
pub(super) mod renderer;

mod buffer;
