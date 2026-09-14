#![allow(clippy::too_many_arguments)]
use crate::render::upload::scene::SceneUploadStaging;

use crate::render::output::RenderTargetId;
use crate::render::scene_resources::SceneResourcePool;
mod execution_adapter;

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
        gpu_plan::{FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH, GpuBufferLengths},
        image::Image,
        image_resource::{
            GpuImageResourceUpload, ImageKey, ImageResourceStore, ImageResourceUploadSignature,
        },
        layer::filter::{self as filter_model, Filter},
        line_seg::LineSegment,
        tile_seg_range::TileSegmentRange,
    },
    text::{PreparedTextData, TextContext},
};

use super::buffer::{WgpuBuffer, WgpuRangeScatterPipeline};
use super::canvas::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers};
use super::coarse::WgpuCoarsePipeline;
use super::commands::WgpuCommandBatch;
use super::cumsum::WgpuCumsumPipeline;
use super::filter::{
    FILTER_OPACITY, WgpuFilterBrushBindings, WgpuFilterPathBindings, WgpuFilterPipeline,
    WgpuFilterTurbulenceBindings,
};
use super::filter_resources::{
    WgpuFilterBrushBuffers, WgpuFilterConvolveBuffers, WgpuFilterPathBuffers,
    WgpuFilterTransferBuffers, WgpuFilterTurbulenceBuffers,
};
use super::filter_work::{FilterTileWork, FilterTileWorkArena};
use super::fine::{WgpuFinePipeline, premul_clear_color};
use super::image_resources::large_texture_table_len;
use super::lazy::PipelineCompilationTracker;
use super::profile::{WgpuRenderProfile, WgpuRenderProfiler, profile_cpu, start_cpu_scope};
use super::scan::WgpuScanPipeline;
use super::target::WgpuTarget;
use crate::render::binning::prefer_dense_binning;
use crate::render::coarse::CoarseBatch;
use crate::render::damage_tiles::DamageTiles;
use crate::render::filter_resources::cursors::FilterCursors;
use crate::render::incremental::{
    ActiveScanPlan, CoarseBinningMode, IncrementalRenderConfig, IncrementalRenderStats,
};

mod composite;
mod debug;
mod execute;
mod filter_ops;
mod lifecycle;
mod output;
mod prepare;
mod resources;
mod scene;
mod targets;
mod vector_images;

use crate::render::retained::{HistoryOwner, RetainedRenderState, SelectedScene};
pub use output::WgpuTextureRenderError;

/// WGPU resources and compute pipelines for the shared rendering algorithms.
#[derive(Clone, Debug, Default)]
pub struct RendererOptions {
    /// Shared backend pipeline cache used by every compute pipeline created by this renderer.
    ///
    /// The cache must have been created from `device`. The caller owns loading and persisting its
    /// data because only the application knows the appropriate cache directory and lifetime.
    pub pipeline_cache: Option<::wgpu::PipelineCache>,
}

/// WGPU resources and compute pipelines for the shared rendering algorithms.
pub struct Renderer {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    options: RendererOptions,
    vector_images: crate::render::vector_images::VectorImageCache<Renderer>,
    lengths: GpuBufferLengths,
    plan: Option<Rc<ExecPlan>>,
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    range_scatter_pipeline: Rc<WgpuRangeScatterPipeline>,
    scene_upload: SceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    max_clip_depth: usize,
    max_group_depth: usize,
    fine_spills: WgpuBuffer,
    text_data: Option<PreparedTextData>,
    scan_pipeline: Option<WgpuScanPipeline>,
    cumsum: Option<WgpuCumsumPipeline>,
    coarse_pipeline: Option<WgpuCoarsePipeline>,
    fine: Option<WgpuFinePipeline>,
    filter: Option<WgpuFilterPipeline>,
    pipeline_compilations: PipelineCompilationTracker,
    filter_transfers: WgpuFilterTransferBuffers,
    filter_brushes: WgpuFilterBrushBuffers,
    scene_preparation: crate::render::prepare::ScenePreparation,
    prepared_output_read_usages: ::wgpu::TextureUsages,
    retained: RetainedRenderState<WgpuTarget>,
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
    local_scene_resources: SceneResourcePool<WgpuSceneAllocation>,
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

/// WGPU objects; matching-size reuse and quarantine live in the shared pool.
struct WgpuSceneAllocation {
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    scene_upload: SceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    fine_spills: WgpuBuffer,
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

type SceneResources = crate::render::scene_resources::SceneResources<WgpuSceneAllocation>;

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

#[cfg(test)]
mod tests;

#[cfg(feature = "bench-internals")]
mod image_upload_benchmark;
#[cfg(feature = "bench-internals")]
pub use image_upload_benchmark::{ImageResourceUploadBenchmark, PreparedImageResourceUpload};

mod frame_adapter;
