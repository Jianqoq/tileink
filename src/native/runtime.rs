//! Native API adapters execute shared Canvas/retained compute batches.
pub(super) mod adapter;
#[cfg(feature = "dx12")]
mod dx12;
#[cfg(test)]
#[path = "runtime/tests/isolation.rs"]
mod isolation;
#[cfg(all(test, not(feature = "metal")))]
mod lifecycle_gpu_tests;
#[cfg(feature = "metal")]
mod metal;
#[cfg(all(test, feature = "metal"))]
mod metal_lifecycle_gpu_tests;
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
