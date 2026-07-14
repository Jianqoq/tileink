#![allow(clippy::too_many_arguments)]

use std::rc::Rc;

use peniko::Color;

use crate::{
    RetainedScene, SceneVersion, TextFontSystem,
    canvas::Canvas,
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    retained_scene::PersistentSceneMaterializer,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
        gpu_plan::{
            FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH, GpuBufferLengths, filter_scratch_extra,
            required_scratch_count,
        },
        image::Image,
        image_resource::{
            GpuImageResourceUpload, ImageKey, ImageResourceStore, ImageResourceUploadSignature,
        },
        layer::{
            Layer,
            filter::{self as filter_model, Filter},
        },
        line_seg::LineSegment,
        offscreen::{local_filter, local_offscreen_scene},
        tile_seg_range::TileSegmentRange,
    },
    text::{PreparedTextData, TextContext},
};

use super::buffer::{WgpuBuffer, WgpuRangeScatterPipeline};
use super::canvas::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuSceneUploadStaging};
use super::coarse::{WgpuCoarseBatch, WgpuCoarsePipeline, prefer_dense_binning};
use super::commands::WgpuCommandBatch;
use super::cumsum::WgpuCumsumPipeline;
use super::damage_tiles::DamageTiles;
use super::filter::{
    FILTER_OPACITY, WgpuFilterBrushBindings, WgpuFilterPathBindings, WgpuFilterPipeline,
    WgpuFilterTurbulenceBindings, region_bounds,
};
use super::filter_resources::{
    WgpuFilterBrushBuffers, WgpuFilterConvolveBuffers, WgpuFilterCursors, WgpuFilterPathBuffers,
    WgpuFilterTransferBuffers, WgpuFilterTurbulenceBuffers,
};
use super::filter_work::{FilterTileWork, FilterTileWorkArena};
use super::fine::{WgpuFinePipeline, premul_clear_color};
use super::image_resources::large_texture_table_len;
use super::incremental::{
    ActiveScanPlan, CoarseBinningMode, IncrementalRenderConfig, IncrementalRenderStats,
};
use super::lazy::PipelineCompilationTracker;
use super::profile::{WgpuRenderProfile, WgpuRenderProfiler, profile_cpu, start_cpu_scope};
use super::retained_surfaces::{RetainedSurface, RetainedSurfaceKind, RetainedSurfaceMeta};
use super::scan::WgpuScanPipeline;
use super::target::WgpuTarget;

mod composite;
mod debug;
mod execute;
mod filter_ops;
mod layers;
mod lifecycle;
mod output;
mod prepare;
mod resources;
mod retained;
mod scene;
mod targets;

pub use output::{ExternalTextureHistoryId, WgpuTextureRenderError};
use retained::{HistoryOwner, RetainedRenderState, SelectedScene};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WgpuRenderTargetId {
    Main,
    Scratch(usize),
}

/// Wgpu-owned renderer target and native compute pipelines.
#[derive(Clone, Debug, Default)]
pub struct RendererOptions {
    /// Shared backend pipeline cache used by every compute pipeline created by this renderer.
    ///
    /// The cache must have been created from `device`. The caller owns loading and persisting its
    /// data because only the application knows the appropriate cache directory and lifetime.
    pub pipeline_cache: Option<::wgpu::PipelineCache>,
}

/// Wgpu-owned renderer target and native compute pipelines.
pub struct Renderer {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    lengths: GpuBufferLengths,
    plan: Option<Rc<ExecPlan>>,
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    range_scatter_pipeline: Rc<WgpuRangeScatterPipeline>,
    scene_upload: WgpuSceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    max_clip_depth: usize,
    max_group_depth: usize,
    fine_spills: WgpuBuffer,
    fine_indirect_args: WgpuBuffer,
    text_data: Option<PreparedTextData>,
    scan_pipeline: Option<WgpuScanPipeline>,
    cumsum: Option<WgpuCumsumPipeline>,
    coarse_pipeline: Option<WgpuCoarsePipeline>,
    fine: Option<WgpuFinePipeline>,
    filter: Option<WgpuFilterPipeline>,
    pipeline_compilations: PipelineCompilationTracker,
    filter_transfers: WgpuFilterTransferBuffers,
    filter_brushes: WgpuFilterBrushBuffers,
    prepared_plan_fingerprint: Option<u64>,
    retained: RetainedRenderState,
    filter_tile_work_arena: FilterTileWorkArena,
    image_resources: ImageResourceStore,
    image_resource_upload: GpuImageResourceUpload,
    image_resource_upload_signature: ImageResourceUploadSignature,
    image_resource_texture_table_len: u32,
    image_resources_dirty: bool,
    filter_convolves: WgpuFilterConvolveBuffers,
    filter_turbulence: WgpuFilterTurbulenceBuffers,
    filter_paths: WgpuFilterPathBuffers,
    // Compatibility surface for render()/image(); render_to_wgpu_texture writes caller-owned textures directly.
    readback_target: WgpuTarget,
    fine_portable_source: WgpuTarget,
    fine_portable_target: WgpuTarget,
    filter_target_snapshot: WgpuTarget,
    root_target_texture: Option<::wgpu::Texture>,
    root_target_view: Option<::wgpu::TextureView>,
    scratch: Vec<WgpuTarget>,
    scratch_spares: Vec<WgpuTarget>,
    scratch_in_use: Vec<bool>,
    local_scene_resource_pool: Vec<SceneResources>,
    pending_local_scene_resources: Vec<SceneResources>,
    #[cfg(feature = "bench-internals")]
    reuse_local_scene_resources: bool,
    clear_color: u32,
    profiler: WgpuRenderProfiler,
    last_frame_used_native: bool,
    size: (u32, u32),
    surface_origin: (i32, i32),
    persistent_scene: Option<PersistentSceneMaterializer>,
    persistent_scene_rendered: Option<(u64, SceneVersion)>,
}

/// Scene-bound GPU allocations that are temporarily replaced while rendering an offscreen layer.
///
/// A renderer quarantines restored allocations until the current command batch is submitted, then
/// makes them available at the start of the next frame. Buffers retain their grown capacities and
/// fixed render targets are resized only when a later local scene has different dimensions.
struct SceneResources {
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    scene_upload: WgpuSceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    fine_spills: WgpuBuffer,
    fine_indirect_args: WgpuBuffer,
    filter_transfers: WgpuFilterTransferBuffers,
    filter_brushes: WgpuFilterBrushBuffers,
    filter_convolves: WgpuFilterConvolveBuffers,
    filter_turbulence: WgpuFilterTurbulenceBuffers,
    filter_paths: WgpuFilterPathBuffers,
    readback_target: WgpuTarget,
    fine_portable_source: WgpuTarget,
    fine_portable_target: WgpuTarget,
    filter_target_snapshot: WgpuTarget,
    root_target_texture: Option<::wgpu::Texture>,
    root_target_view: Option<::wgpu::TextureView>,
    scratch: Vec<WgpuTarget>,
    scratch_spares: Vec<WgpuTarget>,
    scratch_in_use: Vec<bool>,
}

struct SavedRendererState {
    lengths: GpuBufferLengths,
    plan: Option<Rc<ExecPlan>>,
    resources: SceneResources,
    max_clip_depth: usize,
    max_group_depth: usize,
    size: (u32, u32),
    surface_origin: (i32, i32),
    active_tiles: Option<DamageTiles>,
    filter_active_tile_work: Option<FilterTileWork>,
}

struct WgpuDebugScanReadback {
    backdrops: Vec<i32>,
    tile_segment_ranges: Vec<TileSegmentRange>,
    segments: Vec<LineSegment>,
}

fn copy_texture(
    encoder: &mut ::wgpu::CommandEncoder,
    source: &::wgpu::Texture,
    target: &::wgpu::Texture,
    size: (u32, u32),
) {
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        target.as_image_copy(),
        ::wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
    );
}

fn copy_texture_region(
    encoder: &mut ::wgpu::CommandEncoder,
    source: &::wgpu::Texture,
    target: &::wgpu::Texture,
    size: (u32, u32),
    bounds: Bounds,
) {
    let bounds = bounds.intersect(Bounds::canvas(size.0, size.1));
    if bounds.is_empty() {
        return;
    }
    let origin = ::wgpu::Origin3d {
        x: bounds.x0 as u32,
        y: bounds.y0 as u32,
        z: 0,
    };
    let mut source_copy = source.as_image_copy();
    source_copy.origin = origin;
    let mut target_copy = target.as_image_copy();
    target_copy.origin = origin;
    encoder.copy_texture_to_texture(
        source_copy,
        target_copy,
        ::wgpu::Extent3d {
            width: bounds.width(),
            height: bounds.height(),
            depth_or_array_layers: 1,
        },
    );
}

fn encode_morphology_operator(operator: filter_model::MorphologyOperator) -> u32 {
    match operator {
        filter_model::MorphologyOperator::Erode => 0,
        filter_model::MorphologyOperator::Dilate => 1,
    }
}

fn rect_liquid_glass_region(
    region: Option<&crate::shared::layer::region::Region>,
    fallback_bounds: Bounds,
) -> Option<filter_model::RectLiquidGlassRegion> {
    match region {
        Some(crate::shared::layer::region::Region::Rect { .. }) | None => Some(
            filter_model::rect_liquid_glass_region(region, fallback_bounds),
        ),
        Some(crate::shared::layer::region::Region::Path { .. }) => None,
    }
}

fn draw_bounds(canvas: &Canvas, draw_ix: usize) -> Bounds {
    let bounds = canvas.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

fn tile_count_for_bounds(bounds: Bounds) -> u32 {
    if bounds.is_empty() {
        return 0;
    }
    let width = (bounds.x1.max(0) as u32).div_ceil(crate::TILE_SIZE)
        - (bounds.x0.max(0) as u32 / crate::TILE_SIZE);
    let height = (bounds.y1.max(0) as u32).div_ceil(crate::TILE_SIZE)
        - (bounds.y0.max(0) as u32 / crate::TILE_SIZE);
    width.saturating_mul(height)
}

#[cfg(test)]
mod tests;
