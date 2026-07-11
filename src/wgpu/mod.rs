mod buffer;
mod canvas;
mod coarse;
mod commands;
mod cumsum;
mod filter;
mod filter_resources;
mod filter_work;
mod fine;
mod image_resources;
mod incremental;
mod lazy;
mod profile;
mod renderer;
mod retained_surfaces;
mod scan;
mod target;

pub use incremental::{
    FullRedrawReason, IncrementalOutputMode, IncrementalRenderConfig, IncrementalRenderMode,
    IncrementalRenderStats,
};
pub use profile::{
    WgpuRenderProfile, WgpuRenderProfileEntry, WgpuRenderProfileEventSummary,
    WgpuRenderProfileReport,
};
pub use renderer::{ExternalTextureHistoryId, Renderer, RendererOptions, WgpuTextureRenderError};

/// Linear compute workloads use two dimensions once one device dimension is exhausted. Shaders
/// that use this helper must linearize `workgroup_id` with `num_workgroups` in the same order.
fn dispatch_2d(workgroups: u32, maximum_dimension: u32) -> (u32, u32) {
    assert!(workgroups > 0 && maximum_dimension > 0);
    let x = workgroups.min(maximum_dimension);
    let y = workgroups.div_ceil(x);
    assert!(
        y <= maximum_dimension,
        "compute dispatch exceeds the device's 2D workgroup capacity"
    );
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::dispatch_2d;

    #[test]
    fn oversized_linear_dispatch_is_split_across_two_dimensions() {
        assert_eq!(dispatch_2d(96_000, 65_535), (65_535, 2));
        assert_eq!(dispatch_2d(65_535, 65_535), (65_535, 1));
    }
}
