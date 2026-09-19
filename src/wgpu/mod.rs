mod buffer;
mod canvas;
mod coarse;
mod commands;
mod cumsum;
mod dxil;
mod dxil_manifest;
mod filter;
mod filter_resources;
mod filter_work;
mod fine;
mod image_resources;
mod lazy;
mod profile;
mod renderer;
mod scan;
pub(crate) mod shader_variants;
mod target;

pub use profile::{
    WgpuRenderProfile, WgpuRenderProfileEntry, WgpuRenderProfileEventSummary,
    WgpuRenderProfileReport,
};
pub use renderer::{Renderer, RendererOptions, WgpuTextureRenderError};

#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub use renderer::{ImageResourceUploadBenchmark, PreparedImageResourceUpload};

#[cfg(feature = "bench-internals")]
pub use filter::FilterCompilationBenchmark;

#[cfg(test)]
#[path = "../../examples/common/benchmark_gpu.rs"]
pub(crate) mod test_gpu;

mod texture_order;
