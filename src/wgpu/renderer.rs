#![allow(clippy::too_many_arguments)]

use std::sync::{Arc as SharedArc, mpsc};

use peniko::Color;

use crate::{
    TextFontSystem,
    canvas::{Canvas, RetainedFrame, RetainedSceneCache},
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, ROOT_COMMAND_LIST_ID},
        gpu_coarse::{FINE_TILE_DISPATCH_WORDS, FINE_TILE_LIST_COUNT},
        gpu_plan::{
            FINE_GROUP_SPILL_FIELDS, FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH,
            FINE_WORKGROUP_SIZE, GpuBufferLengths, GpuCanvasConfig, filter_scratch_extra,
            plan_stack_depths, required_scratch_count,
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

use super::buffer::WgpuBuffer;
use super::canvas::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuSceneUploadStaging};
use super::coarse::{WgpuCoarseBatch, WgpuCoarsePipeline};
use super::commands::WgpuCommandBatch;
use super::cumsum::WgpuCumsumPipeline;
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
    ActiveScanPlan, DamagePlan, DamageTiles, IncrementalRenderConfig, IncrementalRenderStats,
    IncrementalState,
};
use super::lazy::PipelineCompilationTracker;
use super::profile::{WgpuRenderProfile, WgpuRenderProfiler, profile_cpu, start_cpu_scope};
use super::retained_surfaces::{
    RetainedSurface, RetainedSurfaceCache, RetainedSurfaceKind, RetainedSurfaceMeta,
};
use super::scan::WgpuScanPipeline;
use super::target::WgpuTarget;

mod filter_ops;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WgpuRenderTargetId {
    Main,
    Scratch(usize),
}

#[derive(Debug)]
pub enum WgpuTextureRenderError {
    DestinationTooSmall {
        required_width: u32,
        required_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    DestinationUsageMissing(::wgpu::TextureUsages),
    DestinationStorageUsageMissing(::wgpu::TextureUsages),
    UnsupportedDestination {
        format: ::wgpu::TextureFormat,
        dimension: ::wgpu::TextureDimension,
        sample_count: u32,
    },
}

impl std::fmt::Display for WgpuTextureRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DestinationTooSmall {
                required_width,
                required_height,
                actual_width,
                actual_height,
            } => write!(
                f,
                "destination texture is {actual_width}x{actual_height}, but {required_width}x{required_height} is required"
            ),
            Self::DestinationUsageMissing(usage) => write!(
                f,
                "destination texture usage {usage:?} is missing wgpu::TextureUsages::COPY_DST"
            ),
            Self::DestinationStorageUsageMissing(usage) => write!(
                f,
                "destination texture usage {usage:?} is missing wgpu::TextureUsages::STORAGE_BINDING"
            ),
            Self::UnsupportedDestination {
                format,
                dimension,
                sample_count,
            } => write!(
                f,
                "unsupported destination texture format {format:?}, dimension {dimension:?}, sample_count {sample_count}; expected single-sample 2D Rgba8Unorm or Rgba8UnormSrgb"
            ),
        }
    }
}

impl std::error::Error for WgpuTextureRenderError {}

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
    plan: Option<ExecPlan>,
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
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
    retained_scene_cache: RetainedSceneCache,
    retained_materialized: Option<CachedMaterializedScene>,
    prepared_retained_scene: Option<*const Canvas>,
    prepared_retained_uses_text: bool,
    prepared_plan_fingerprint: Option<u64>,
    incremental_config: IncrementalRenderConfig,
    incremental_state: IncrementalState,
    incremental_stats: IncrementalRenderStats,
    active_tiles: Option<DamageTiles>,
    filter_tile_work_arena: FilterTileWorkArena,
    history_valid: bool,
    retained_surfaces: RetainedSurfaceCache,
    rendering_frame: Option<RetainedFrame>,
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
    scratch_in_use: Vec<bool>,
    clear_color: u32,
    profiler: WgpuRenderProfiler,
    last_frame_used_native: bool,
    size: (u32, u32),
    surface_origin: (i32, i32),
}

struct CachedMaterializedScene {
    frame: RetainedFrame,
    scene: SharedArc<Canvas>,
}

enum SelectedScene<'a> {
    Borrowed(&'a Canvas),
    Retained {
        scene: SharedArc<Canvas>,
        frame: RetainedFrame,
    },
}

impl SelectedScene<'_> {
    fn scene(&self) -> &Canvas {
        match self {
            Self::Borrowed(scene) => scene,
            Self::Retained { scene, .. } => scene,
        }
    }

    fn frame(&self) -> Option<RetainedFrame> {
        match self {
            Self::Borrowed(_) => None,
            Self::Retained { frame, .. } => Some(frame.clone()),
        }
    }

    fn retained_ptr(&self) -> Option<*const Canvas> {
        match self {
            Self::Borrowed(_) => None,
            Self::Retained { scene, .. } => Some(SharedArc::as_ptr(scene)),
        }
    }
}

struct SavedRendererState {
    lengths: GpuBufferLengths,
    plan: Option<ExecPlan>,
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    scene_upload: WgpuSceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    max_clip_depth: usize,
    max_group_depth: usize,
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
    scratch_in_use: Vec<bool>,
    size: (u32, u32),
    surface_origin: (i32, i32),
    active_tiles: Option<DamageTiles>,
    filter_active_tile_work: Option<FilterTileWork>,
}

impl Render for Renderer {
    fn render(&mut self, canvas: &Canvas) {
        assert!(
            self.render_native(canvas),
            "wgpu renderer could not render scene natively"
        );
    }
}

impl Renderer {
    pub fn new(
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Self {
        Self::new_with_options(
            device,
            queue,
            width,
            height,
            clear,
            RendererOptions::default(),
        )
    }

    pub fn new_with_options(
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        width: u32,
        height: u32,
        clear: Color,
        options: RendererOptions,
    ) -> Self {
        let pipeline_cache = options.pipeline_cache.as_ref();
        let pipeline_compilations = PipelineCompilationTracker::default();
        Self {
            device: device.clone(),
            queue: queue.clone(),
            lengths: GpuBufferLengths::default(),
            plan: None,
            config: WgpuBuffer::new(device, "tileink wgpu canvas config"),
            scene_buffers: WgpuSceneBuffers::new(device),
            scene_upload: WgpuSceneUploadStaging::default(),
            scan: WgpuScanBuffers::new(device),
            coarse: WgpuCoarseBuffers::new(device),
            max_clip_depth: 0,
            max_group_depth: 0,
            fine_spills: WgpuBuffer::new(device, "tileink wgpu fine spills"),
            fine_indirect_args: WgpuBuffer::new(device, "tileink wgpu fine indirect args"),
            text_data: None,
            scan_pipeline: WgpuScanPipeline::new(device, pipeline_cache, &pipeline_compilations),
            cumsum: WgpuCumsumPipeline::new(device, pipeline_cache, &pipeline_compilations),
            coarse_pipeline: WgpuCoarsePipeline::new(
                device,
                pipeline_cache,
                &pipeline_compilations,
            ),
            fine: WgpuFinePipeline::new(device, pipeline_cache, &pipeline_compilations),
            filter: WgpuFilterPipeline::new(device, pipeline_cache, &pipeline_compilations),
            pipeline_compilations,
            filter_transfers: WgpuFilterTransferBuffers::new(device),
            filter_brushes: WgpuFilterBrushBuffers::new(device),
            retained_scene_cache: RetainedSceneCache::default(),
            retained_materialized: None,
            prepared_retained_scene: None,
            prepared_retained_uses_text: false,
            prepared_plan_fingerprint: None,
            incremental_config: IncrementalRenderConfig::default(),
            incremental_state: IncrementalState::default(),
            incremental_stats: IncrementalRenderStats::default(),
            active_tiles: None,
            filter_tile_work_arena: FilterTileWorkArena::default(),
            history_valid: false,
            retained_surfaces: RetainedSurfaceCache::new(
                IncrementalRenderConfig::default().retained_texture_budget_bytes,
            ),
            rendering_frame: None,
            image_resources: ImageResourceStore::default(),
            image_resource_upload: GpuImageResourceUpload::default(),
            image_resource_upload_signature: ImageResourceUploadSignature::default(),
            image_resource_texture_table_len: large_texture_table_len(device),
            image_resources_dirty: true,
            filter_convolves: WgpuFilterConvolveBuffers::new(device),
            filter_turbulence: WgpuFilterTurbulenceBuffers::new(device),
            filter_paths: WgpuFilterPathBuffers::new(device),
            readback_target: WgpuTarget::new(device, width, height),
            fine_portable_source: WgpuTarget::new(device, width, height),
            fine_portable_target: WgpuTarget::new(device, width, height),
            filter_target_snapshot: WgpuTarget::new(device, width, height),
            root_target_texture: None,
            root_target_view: None,
            scratch: Vec::new(),
            scratch_in_use: Vec::new(),
            clear_color: premul_clear_color(clear),
            profiler: WgpuRenderProfiler::default(),
            last_frame_used_native: true,
            size: (width, height),
            surface_origin: (0, 0),
        }
    }

    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        let instance =
            ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
                power_preference: ::wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .expect("request default wgpu adapter");
        let required_features = adapter.features()
            & (::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | ::wgpu::Features::TIMESTAMP_QUERY
                | ::wgpu::Features::TEXTURE_BINDING_ARRAY
                | ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
                label: Some("tileink default wgpu device"),
                required_features,
                required_limits: adapter.limits(),
                memory_hints: ::wgpu::MemoryHints::MemoryUsage,
                trace: ::wgpu::Trace::Off,
                experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
            }))
            .expect("request default wgpu device");
        Self::new(&device, &queue, width, height, clear)
    }

    pub fn render(&mut self, canvas: &Canvas) {
        <Self as Render>::render(self, canvas);
    }

    pub fn insert_image(&mut self, key: ImageKey, image: impl Into<SharedArc<Image>>) -> bool {
        let image = image.into();
        if !self.image_resources.insert(key, image.clone()) {
            return false;
        }
        self.image_resources_dirty = true;
        self.invalidate_retained_history();
        true
    }

    pub fn remove_image(&mut self, key: ImageKey) -> bool {
        let removed = self.image_resources.remove(key);
        if removed {
            self.image_resources_dirty = true;
            self.invalidate_retained_history();
            return true;
        }
        false
    }

    pub fn clear_images(&mut self) -> bool {
        let removed = self.image_resources.clear();
        if removed {
            self.image_resources_dirty = true;
            self.invalidate_retained_history();
            return true;
        }
        false
    }

    pub fn image_resource(&self, key: ImageKey) -> Option<&Image> {
        self.image_resources.get(key)
    }

    pub fn render_profiled(&mut self, canvas: &Canvas) -> WgpuRenderProfile {
        self.start_profile();
        self.render(canvas);
        self.end_profile().clone()
    }

    pub fn render_with_text_profiled(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) -> WgpuRenderProfile {
        self.start_profile();
        self.render_with_text(canvas, font_system, text_context);
        self.end_profile().clone()
    }

    /// Starts collecting CPU stage timings and GPU pass timestamps.
    ///
    /// GPU durations require a device created with `wgpu::Features::TIMESTAMP_QUERY`.
    /// `new_default_device` requests that feature when the adapter supports it.
    /// Any unresolved GPU timestamp readbacks from the previous profile are discarded so stale
    /// async results cannot be attached to the wrong frame.
    pub fn start_profile(&mut self) {
        self.profiler.start(&self.device);
    }

    /// Stops profiling, starts async timestamp readback, and returns the latest CPU profile.
    ///
    /// GPU timestamp entries are merged by `poll_profile` after the device has made the mapped
    /// readback buffers available. This keeps `end_profile` off the GPU completion path.
    pub fn end_profile(&mut self) -> &WgpuRenderProfile {
        self.profiler.end(&self.device, &self.queue)
    }

    /// Polls pending async GPU timestamp readbacks without blocking and returns the latest profile.
    pub fn poll_profile(&mut self) -> &WgpuRenderProfile {
        self.profiler.poll_ready(&self.device)
    }

    pub fn has_pending_profile_readbacks(&self) -> bool {
        self.profiler.has_pending_readbacks()
    }

    pub fn profile(&self) -> &WgpuRenderProfile {
        self.profiler.profile()
    }

    /// Whether the last `render*_to_wgpu_texture` call used native wgpu compute.
    pub fn last_frame_used_native_gpu(&self) -> bool {
        self.last_frame_used_native
    }

    pub fn incremental_render_config(&self) -> IncrementalRenderConfig {
        self.incremental_config
    }

    pub fn set_incremental_render_config(&mut self, config: IncrementalRenderConfig) {
        self.incremental_config = config.validate();
        self.retained_surfaces
            .set_budget(self.incremental_config.retained_texture_budget_bytes);
    }

    pub fn incremental_render_stats(&self) -> &IncrementalRenderStats {
        &self.incremental_stats
    }

    /// Monotonic epoch advanced after each lazy compute pipeline is compiled.
    /// Applications can use this to persist a backend pipeline cache only when
    /// new data may have been added, without polling the cache every frame.
    pub fn pipeline_compilation_epoch(&self) -> u64 {
        self.pipeline_compilations.epoch()
    }

    /// Invalidates the retained history after external state changes that the
    /// scene graph cannot associate with a specific node.
    pub fn invalidate_retained_history(&mut self) {
        self.incremental_state.invalidate_renderer_state();
        self.history_valid = false;
        self.prepared_retained_scene = None;
    }

    /// Updates the clear color without rebuilding device-owned pipelines, so one renderer can
    /// render multiple scenes/examples in a single process.
    pub fn set_clear_color(&mut self, clear: Color) {
        let clear = premul_clear_color(clear);
        if self.clear_color != clear {
            self.clear_color = clear;
            self.invalidate_retained_history();
        }
    }

    /// Renders only through native wgpu compute pipelines.
    ///
    /// This is useful for tests that need to verify the native WGPU path directly.
    pub fn render_native(&mut self, canvas: &Canvas) -> bool {
        let selected = self.select_scene(canvas);
        let frame = selected.frame();
        let retained_ptr = selected.retained_ptr();
        let scene = selected.scene();
        let plan = self.begin_incremental_frame(frame, scene);
        let has_work = !plan.tiles.is_empty();
        if has_work && self.scene_needs_prepare(retained_ptr, false) {
            self.prepare_scene(scene);
            self.mark_scene_prepared(retained_ptr, false);
        }
        let rendered = !has_work || self.render_prepared_native(scene);
        self.finish_incremental_frame(plan, rendered);
        rendered
    }

    /// Renders text scenes only through native wgpu compute pipelines.
    ///
    /// The caller supplies the text context so backend parity tests can compare
    /// native wgpu against another backend using exactly the same glyph atlas.
    pub fn render_native_with_text(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) -> bool {
        let selected = self.select_scene(canvas);
        let frame = selected.frame();
        let retained_ptr = selected.retained_ptr();
        let scene = selected.scene();
        let plan = self.begin_incremental_frame(frame, scene);
        let has_work = !plan.tiles.is_empty();
        if has_work && self.scene_needs_prepare(retained_ptr, true) {
            self.prepare_scene_with_text(scene, font_system, text_context);
            self.mark_scene_prepared(retained_ptr, true);
        }
        let rendered = !has_work || self.render_prepared_native(scene);
        self.finish_incremental_frame(plan, rendered);
        rendered
    }

    fn select_scene<'a>(&mut self, canvas: &'a Canvas) -> SelectedScene<'a> {
        let Some(frame) = canvas.retained_frame() else {
            self.prepared_retained_scene = None;
            return SelectedScene::Borrowed(canvas);
        };

        if frame.complete
            && let Some(cached) = &self.retained_materialized
            && cached.frame.same_scene(&frame)
        {
            return SelectedScene::Retained {
                scene: cached.scene.clone(),
                frame,
            };
        }

        let scene =
            SharedArc::new(canvas.materialize_retained_scenes(&mut self.retained_scene_cache));
        self.retained_scene_cache.retain_frame(&frame);
        if frame.complete {
            self.retained_materialized = Some(CachedMaterializedScene {
                frame: frame.clone(),
                scene: scene.clone(),
            });
        }
        SelectedScene::Retained { scene, frame }
    }

    fn begin_incremental_frame(
        &mut self,
        frame: Option<RetainedFrame>,
        scene: &Canvas,
    ) -> DamagePlan {
        if self.retained_surfaces.take_backdrop_evicted() {
            self.history_valid = false;
        }
        let physical_size = scene.physical_size();
        let mut plan = self.incremental_state.plan(
            frame,
            physical_size,
            self.incremental_config,
            self.history_valid,
        );
        if !plan.stats.full_redraw {
            let mut dependent = plan.tiles.coalesced_rects(physical_size);
            scene.propagate_damage(&mut dependent);
            plan.include_dependent_bounds(dependent, self.incremental_config);
        }
        self.incremental_stats = plan.stats.clone();
        self.active_tiles = (!plan.stats.full_redraw).then(|| plan.tiles.clone());
        self.rendering_frame = plan.frame.clone();
        plan
    }

    fn finish_incremental_frame(&mut self, plan: DamagePlan, rendered: bool) {
        let backdrop_history_valid = !self.retained_surfaces.take_backdrop_evicted();
        if rendered {
            if let Some(frame) = &plan.frame {
                let nodes = frame.nodes.iter().map(|node| node.id).collect();
                self.retained_surfaces.retain_nodes(&nodes);
            }
            self.incremental_state.commit(plan.frame);
            self.history_valid = backdrop_history_valid;
        } else {
            self.history_valid = false;
        }
        self.active_tiles = None;
        self.rendering_frame = None;
    }

    fn retained_surface_meta(
        &self,
        id: crate::canvas::RetainedSurfaceId,
        kind: RetainedSurfaceKind,
        size: (u32, u32),
        origin: (i32, i32),
        bounds: Bounds,
    ) -> Option<RetainedSurfaceMeta> {
        Some(RetainedSurfaceMeta {
            revision: self.rendering_frame.as_ref()?.node_revision(id.node)?,
            kind,
            size,
            origin,
            bounds,
        })
    }

    fn retained_surface_is_dirty(&self, bounds: Bounds) -> bool {
        self.active_tiles
            .as_ref()
            .is_none_or(|tiles| tiles.intersects_bounds(bounds))
    }

    fn local_damage_for_surface(&self, surface: Bounds) -> Option<DamageTiles> {
        let active = self.active_tiles.as_ref()?;
        let mut local = DamageTiles::new((surface.width(), surface.height()));
        for bounds in active.coalesced_rects(self.size) {
            let bounds = bounds.intersect(surface);
            if !bounds.is_empty() {
                local.add_bounds(Bounds::new(
                    bounds.x0 - surface.x0,
                    bounds.y0 - surface.y0,
                    bounds.x1 - surface.x0,
                    bounds.y1 - surface.y0,
                ));
            }
        }
        Some(local)
    }

    fn prepare_active_tile_buffers(&mut self) {
        if let Some(active) = &self.active_tiles {
            self.coarse
                .upload_active_tiles(&self.queue, self.lengths, active.list());
        }
        self.prepare_filter_active_tile_work();
    }

    /// Uploads the current compact filter worklist into a unique arena slot.
    ///
    /// A frame may switch from an expanded source/halo list to an exact output
    /// list after commands using the first list have already been encoded.
    /// Separate buffers prevent a later queue write from changing what those
    /// earlier dispatches observe; slots are retained and reused next frame.
    fn prepare_filter_active_tile_work(&mut self) {
        let Some(tiles) = self
            .active_tiles
            .as_ref()
            .map(|active| active.list().to_vec())
        else {
            if let Some(filter) = self.filter.as_mut() {
                filter.clear_active_tile_work();
            }
            return;
        };
        self.prepare_filter_tile_work(&tiles);
    }

    fn prepare_filter_tile_work(&mut self, tiles: &[u32]) {
        let work = self
            .filter_tile_work_arena
            .upload(&self.device, &self.queue, tiles);
        if let Some(filter) = self.filter.as_mut() {
            filter.restore_active_tile_work(Some(work));
        }
    }

    fn take_matching_retained_surface(
        &mut self,
        id: Option<crate::canvas::RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
    ) -> Option<(crate::canvas::RetainedSurfaceId, RetainedSurface)> {
        let id = id?;
        let surface = self.retained_surfaces.take(id)?;
        if Some(surface.meta) == meta {
            Some((id, surface))
        } else {
            None
        }
    }

    fn cache_retained_surface(
        &mut self,
        id: Option<crate::canvas::RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
        primary: WgpuTarget,
        secondary: Option<WgpuTarget>,
        backdrop_source: Option<WgpuTarget>,
    ) {
        if let (Some(id), Some(meta)) = (id, meta) {
            self.retained_surfaces
                .insert(id, meta, primary, secondary, backdrop_source);
        }
    }

    fn scene_needs_prepare(&self, retained_ptr: Option<*const Canvas>, uses_text: bool) -> bool {
        retained_ptr.is_none()
            || self.prepared_retained_scene != retained_ptr
            || self.prepared_retained_uses_text != uses_text
            || self.image_resources_dirty
    }

    fn mark_scene_prepared(&mut self, retained_ptr: Option<*const Canvas>, uses_text: bool) {
        self.prepared_retained_scene = retained_ptr;
        self.prepared_retained_uses_text = uses_text;
    }

    fn render_prepared_native(&mut self, canvas: &Canvas) -> bool {
        if self.render_prepared_tile_plan(canvas) {
            self.size = (canvas.physical_width(), canvas.physical_height());
            return true;
        }
        false
    }

    fn prepare_scene(&mut self, canvas: &Canvas) {
        let _profile_scope = start_cpu_scope("prepare");
        self.text_data = None;
        self.prepare_scene_resources(canvas);
    }

    fn prepare_scene_with_text(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        let _profile_scope = start_cpu_scope("prepare");
        self.text_data = profile_cpu("prepare.text", || {
            Some(PreparedTextData::new(
                &canvas.text_glyphs,
                &canvas.text_runs,
                font_system,
                text_context,
            ))
        });
        self.prepare_scene_resources(canvas);
    }

    fn prepare_scene_resources(&mut self, canvas: &Canvas) {
        self.size = (canvas.physical_width(), canvas.physical_height());
        self.surface_origin = (0, 0);
        profile_cpu("prepare.target", || {
            if self.root_target_view.is_none() {
                self.readback_target.resize(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                );
            }
            self.fine_portable_source.resize(
                &self.device,
                canvas.physical_width(),
                canvas.physical_height(),
            );
            self.fine_portable_target.resize(
                &self.device,
                canvas.physical_width(),
                canvas.physical_height(),
            );
        });
        let lengths = profile_cpu("prepare.lengths", || {
            self.scene_upload
                .build_lengths(canvas, self.text_data.as_ref())
        });
        let plan_fingerprint = canvas.execution_plan_fingerprint();
        let reused_plan =
            self.prepared_plan_fingerprint == Some(plan_fingerprint) && self.plan.is_some();
        let plan = profile_cpu("prepare.compile", || {
            if reused_plan {
                self.plan.as_ref().expect("cached execution plan").clone()
            } else {
                canvas.compile(ROOT_COMMAND_LIST_ID)
            }
        });
        self.incremental_stats.reused_compiled_plan = reused_plan;
        self.prepared_plan_fingerprint = Some(plan_fingerprint);
        let (max_clip_depth, max_group_depth) =
            profile_cpu("prepare.stack_depths", || plan_stack_depths(&plan));
        profile_cpu("prepare.upload_scene", || {
            self.prepare_image_resource_buffers(canvas.scene_image_resources(), false);
            self.scene_buffers.upload(
                &self.device,
                &self.queue,
                canvas,
                lengths,
                &plan,
                self.text_data.as_ref(),
                Some(&self.image_resource_upload),
                &mut self.scene_upload,
            );
        });
        profile_cpu("prepare.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.coarse_buffers", || {
            profile_cpu("prepare.coarse_buffers.resize", || {
                self.coarse.prepare_outputs(&self.device, lengths);
            });
            profile_cpu("prepare.coarse_buffers.upload_tile_draw_bins", || {
                self.coarse
                    .upload_tile_draw_bins(&self.queue, lengths, &mut self.scene_upload);
            });
        });
        profile_cpu("prepare.fine_spills", || {
            self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        });
        profile_cpu("prepare.scratch", || {
            self.prepare_scratch_buffers(required_scratch_count(&plan));
        });
        profile_cpu("prepare.filter_uploads", || {
            self.filter_transfers
                .upload(&self.device, &self.queue, &plan);
            self.filter_brushes.upload(
                &self.device,
                &self.queue,
                &plan,
                Some(&self.image_resource_upload),
            );
            self.filter_convolves
                .upload(&self.device, &self.queue, &plan);
            self.filter_turbulence
                .upload(&self.device, &self.queue, &plan);
            self.filter_paths.upload(&self.device, &self.queue, &plan);
        });
        profile_cpu("prepare.config", || {
            self.config.upload(
                &self.device,
                &self.queue,
                "tileink wgpu canvas config",
                &[GpuCanvasConfig::new(canvas, lengths, self.clear_color)],
            );
        });
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan);
    }

    fn prepare_image_resource_buffers(
        &mut self,
        scene_resources: &ImageResourceStore,
        force_upload: bool,
    ) {
        let limits = self.device.limits();
        let max_atlas_dimension = limits.max_texture_dimension_2d;
        let max_atlas_pages = limits.max_texture_array_layers;
        let signature = self.image_resources.upload_signature(
            scene_resources,
            max_atlas_dimension,
            max_atlas_pages,
            self.image_resource_texture_table_len,
        );
        let rebuild_upload =
            self.image_resources_dirty || self.image_resource_upload_signature != signature;

        if rebuild_upload {
            self.image_resource_upload = self.image_resources.upload_merged(
                scene_resources,
                max_atlas_dimension,
                max_atlas_pages,
                self.image_resource_texture_table_len,
                Some(&self.image_resource_upload),
            );
            self.image_resource_upload_signature = signature;
            self.image_resources_dirty = false;
        }

        if force_upload || rebuild_upload {
            self.scene_buffers.upload_image_resources(
                &self.device,
                &self.queue,
                &self.image_resource_upload,
                force_upload,
            );
        }
    }

    fn activate_local_scene_resources(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        parent_filter: &Filter,
        scratch_count: usize,
        surface_origin: (i32, i32),
    ) -> SavedRendererState {
        let _profile_scope = start_cpu_scope("prepare.local");
        let saved = SavedRendererState {
            lengths: self.lengths,
            plan: self.plan.take(),
            config: std::mem::replace(
                &mut self.config,
                WgpuBuffer::new(&self.device, "tileink wgpu canvas config"),
            ),
            scene_buffers: std::mem::replace(
                &mut self.scene_buffers,
                WgpuSceneBuffers::new(&self.device),
            ),
            scene_upload: std::mem::take(&mut self.scene_upload),
            scan: std::mem::replace(&mut self.scan, WgpuScanBuffers::new(&self.device)),
            coarse: std::mem::replace(&mut self.coarse, WgpuCoarseBuffers::new(&self.device)),
            max_clip_depth: self.max_clip_depth,
            max_group_depth: self.max_group_depth,
            fine_spills: std::mem::replace(
                &mut self.fine_spills,
                WgpuBuffer::new(&self.device, "tileink wgpu fine spills"),
            ),
            fine_indirect_args: std::mem::replace(
                &mut self.fine_indirect_args,
                WgpuBuffer::new(&self.device, "tileink wgpu fine indirect args"),
            ),
            filter_transfers: std::mem::replace(
                &mut self.filter_transfers,
                WgpuFilterTransferBuffers::new(&self.device),
            ),
            filter_brushes: std::mem::replace(
                &mut self.filter_brushes,
                WgpuFilterBrushBuffers::new(&self.device),
            ),
            filter_convolves: std::mem::replace(
                &mut self.filter_convolves,
                WgpuFilterConvolveBuffers::new(&self.device),
            ),
            filter_turbulence: std::mem::replace(
                &mut self.filter_turbulence,
                WgpuFilterTurbulenceBuffers::new(&self.device),
            ),
            filter_paths: std::mem::replace(
                &mut self.filter_paths,
                WgpuFilterPathBuffers::new(&self.device),
            ),
            readback_target: std::mem::replace(
                &mut self.readback_target,
                WgpuTarget::new(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                ),
            ),
            fine_portable_source: std::mem::replace(
                &mut self.fine_portable_source,
                WgpuTarget::new(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                ),
            ),
            fine_portable_target: std::mem::replace(
                &mut self.fine_portable_target,
                WgpuTarget::new(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                ),
            ),
            filter_target_snapshot: std::mem::replace(
                &mut self.filter_target_snapshot,
                WgpuTarget::new(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                ),
            ),
            root_target_texture: std::mem::take(&mut self.root_target_texture),
            root_target_view: std::mem::take(&mut self.root_target_view),
            scratch: std::mem::take(&mut self.scratch),
            scratch_in_use: std::mem::take(&mut self.scratch_in_use),
            size: self.size,
            surface_origin: self.surface_origin,
            active_tiles: self.active_tiles.take(),
            filter_active_tile_work: self
                .filter
                .as_ref()
                .and_then(WgpuFilterPipeline::active_tile_work),
        };

        let lengths = profile_cpu("prepare.local.lengths", || {
            self.scene_upload
                .build_lengths(canvas, self.text_data.as_ref())
        });
        let (max_clip_depth, max_group_depth) =
            profile_cpu("prepare.local.stack_depths", || plan_stack_depths(plan));
        self.size = (canvas.physical_width(), canvas.physical_height());
        self.surface_origin = surface_origin;
        self.active_tiles = None;
        if let Some(filter) = self.filter.as_mut() {
            filter.clear_active_tile_work();
        }
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan.clone());
        profile_cpu("prepare.local.upload_scene", || {
            self.prepare_image_resource_buffers(canvas.scene_image_resources(), true);
            self.scene_buffers.upload(
                &self.device,
                &self.queue,
                canvas,
                lengths,
                plan,
                self.text_data.as_ref(),
                Some(&self.image_resource_upload),
                &mut self.scene_upload,
            );
        });
        profile_cpu("prepare.local.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.local.coarse_buffers", || {
            profile_cpu("prepare.local.coarse_buffers.resize", || {
                self.coarse.prepare_outputs(&self.device, lengths);
            });
            profile_cpu("prepare.local.coarse_buffers.upload_tile_draw_bins", || {
                self.coarse
                    .upload_tile_draw_bins(&self.queue, lengths, &mut self.scene_upload);
            });
        });
        profile_cpu("prepare.local.fine_spills", || {
            self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        });
        profile_cpu("prepare.local.scratch", || {
            self.prepare_scratch_buffers(scratch_count.max(1));
        });
        profile_cpu("prepare.local.filter_uploads", || {
            self.filter_transfers.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_brushes.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
                Some(&self.image_resource_upload),
            );
            self.filter_convolves.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_turbulence.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_paths.upload(&self.device, &self.queue, plan);
        });
        profile_cpu("prepare.local.config", || {
            self.config.upload(
                &self.device,
                &self.queue,
                "tileink wgpu canvas config",
                &[GpuCanvasConfig::new(canvas, lengths, self.clear_color)],
            );
        });
        saved
    }

    fn restore_root_scene_resources(&mut self, saved: SavedRendererState) {
        self.lengths = saved.lengths;
        self.plan = saved.plan;
        self.config = saved.config;
        self.scene_buffers = saved.scene_buffers;
        self.scene_upload = saved.scene_upload;
        self.scan = saved.scan;
        self.coarse = saved.coarse;
        self.max_clip_depth = saved.max_clip_depth;
        self.max_group_depth = saved.max_group_depth;
        self.fine_spills = saved.fine_spills;
        self.fine_indirect_args = saved.fine_indirect_args;
        self.filter_transfers = saved.filter_transfers;
        self.filter_brushes = saved.filter_brushes;
        self.filter_convolves = saved.filter_convolves;
        self.filter_turbulence = saved.filter_turbulence;
        self.filter_paths = saved.filter_paths;
        self.readback_target = saved.readback_target;
        self.fine_portable_source = saved.fine_portable_source;
        self.fine_portable_target = saved.fine_portable_target;
        self.filter_target_snapshot = saved.filter_target_snapshot;
        self.root_target_texture = saved.root_target_texture;
        self.root_target_view = saved.root_target_view;
        self.scratch = saved.scratch;
        self.scratch_in_use = saved.scratch_in_use;
        self.size = saved.size;
        self.surface_origin = saved.surface_origin;
        self.active_tiles = saved.active_tiles;
        if let Some(filter) = self.filter.as_mut() {
            filter.restore_active_tile_work(saved.filter_active_tile_work);
        }
    }

    fn prepare_scratch_buffers(&mut self, count: usize) {
        while self.scratch.len() < count {
            self.scratch
                .push(WgpuTarget::new(&self.device, self.size.0, self.size.1));
        }
        for scratch in &mut self.scratch {
            scratch.resize(&self.device, self.size.0, self.size.1);
        }
        self.filter_target_snapshot
            .resize(&self.device, self.size.0, self.size.1);
        self.scratch_in_use.clear();
        self.scratch_in_use.resize(self.scratch.len(), false);
    }

    fn prepare_fine_stack_spills(
        &mut self,
        lengths: GpuBufferLengths,
        max_clip_depth: usize,
        max_group_depth: usize,
    ) {
        let lane_count = lengths.tile_count * FINE_WORKGROUP_SIZE as usize;
        let clip_spill_depth = max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH);
        let group_spill_depth = max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH);
        self.fine_spills.resize_uninit::<u32>(
            &self.device,
            "tileink wgpu fine spills",
            lane_count * clip_spill_depth
                + lane_count * group_spill_depth * FINE_GROUP_SPILL_FIELDS,
        );
        self.fine_indirect_args.resize_uninit::<u32>(
            &self.device,
            "tileink wgpu fine indirect args",
            FINE_TILE_LIST_COUNT * FINE_TILE_DISPATCH_WORDS,
        );
    }

    fn scan_and_cumsum(&mut self, commands: &mut WgpuCommandBatch, scene: &Canvas) -> bool {
        let (Some(scan), Some(cumsum)) = (&self.scan_pipeline, &self.cumsum) else {
            return false;
        };
        let active = self
            .active_tiles
            .as_ref()
            .map(|damage| ActiveScanPlan::new(scene, damage));
        if let Some(active) = &active {
            self.scan
                .upload_active_indices(&self.device, &self.queue, &active.indices);
            self.scan
                .upload_active_cumsum_plan(&self.device, &self.queue, &active.cumsum);
            self.incremental_stats.scanned_paths += active.path_count;
            self.incremental_stats.scanned_lines += active.line_count;
            self.incremental_stats.scan_chunks += active.chunk_count;
        } else {
            self.incremental_stats.scanned_paths += self.lengths.path_count as u32;
            self.incremental_stats.scanned_lines += self.lengths.line_count as u32;
            self.incremental_stats.scan_chunks += self.lengths.scan_chunk_count as u32;
        }
        scan.run_in(
            commands,
            &self.scene_buffers,
            &mut self.scan,
            self.lengths,
            active.as_ref(),
        );
        cumsum.run_in(
            commands,
            &self.scene_buffers,
            &mut self.scan,
            self.lengths,
            active.as_ref(),
        );
        true
    }

    #[cfg(test)]
    fn coarse_batch(
        &mut self,
        _canvas: &Canvas,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
    ) {
        if let Some(coarse) = &self.coarse_pipeline {
            coarse.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &self.scan,
                &mut self.coarse,
                self.lengths,
                WgpuCoarseBatch {
                    draw_start,
                    draw_end,
                    layer_stack_start,
                    layer_stack_end,
                    active_tile_count: None,
                },
            );
        }
    }

    #[cfg(test)]
    fn scan_for_test(&mut self) {
        if let Some(scan) = &self.scan_pipeline {
            scan.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        }
    }

    #[cfg(test)]
    fn cumsum_for_test(&mut self) {
        if let Some(cumsum) = &self.cumsum {
            cumsum.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        }
    }

    fn render_prepared_tile_plan(&mut self, canvas: &Canvas) -> bool {
        if self.fine.is_none() || self.coarse_pipeline.is_none() || self.filter.is_none() {
            return false;
        }
        let Some(plan) = self.plan.clone() else {
            return false;
        };

        if self
            .active_tiles
            .as_ref()
            .is_some_and(DamageTiles::is_empty)
        {
            return true;
        }
        if let Some(filter) = &self.filter {
            filter.reset_dispatch_counts();
        }
        self.filter_tile_work_arena.reset();
        self.prepare_active_tile_buffers();

        let mut commands = WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
        if !self.scan_and_cumsum(&mut commands, canvas) {
            return false;
        }
        if self.active_tiles.is_some() {
            self.clear_render_region(
                &mut commands,
                WgpuRenderTargetId::Main,
                Bounds::canvas(self.size.0, self.size.1),
                self.clear_color,
            );
        } else {
            self.clear_render_target(&mut commands, WgpuRenderTargetId::Main, self.clear_color);
        }
        let mut filter_cursors = WgpuFilterCursors::default();
        let ok = self.execute_ops(
            &mut commands,
            canvas,
            &plan,
            &plan.ops,
            WgpuRenderTargetId::Main,
            &mut filter_cursors,
        );
        commands.finish();
        if let Some(filter) = &self.filter {
            let (dispatches, compact_dispatches) = filter.dispatch_counts();
            self.incremental_stats.filter_dispatches = dispatches;
            self.incremental_stats.compact_filter_dispatches = compact_dispatches;
        }
        ok
    }

    fn execute_ops(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        for op in ops {
            let ok = match op {
                ExecOp::DrawBatch { draws, layer_stack } => self.execute_draw_batch(
                    commands,
                    canvas,
                    draws.clone(),
                    layer_stack.clone(),
                    target,
                ),
                ExecOp::BeginClip
                | ExecOp::EndClip
                | ExecOp::BeginOpacity
                | ExecOp::EndOpacity
                | ExecOp::BeginBlend
                | ExecOp::EndBlend => true,
                ExecOp::OffscreenLayer {
                    retained_id,
                    draw,
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    commands,
                    *retained_id,
                    canvas,
                    plan,
                    *draw,
                    layer,
                    outer_stack.clone(),
                    children,
                    target,
                    filter_cursors,
                ),
                ExecOp::OffscreenMaskLayer {
                    retained_id,
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => self.execute_mask_layer(
                    commands,
                    *retained_id,
                    canvas,
                    plan,
                    layer,
                    outer_stack.clone(),
                    content,
                    mask,
                    target,
                    filter_cursors,
                ),
            };
            if !ok {
                return false;
            }
        }
        true
    }

    fn execute_draw_batch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        _scene: &Canvas,
        draws: std::ops::Range<usize>,
        layer_stack: std::ops::Range<usize>,
        target: WgpuRenderTargetId,
    ) -> bool {
        if draws.start >= draws.end {
            return true;
        }
        self.coarse_and_fine_batch_to(
            commands,
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
            target,
        )
    }

    fn coarse_and_fine_batch_to(
        &mut self,
        commands: &mut WgpuCommandBatch,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
        target: WgpuRenderTargetId,
    ) -> bool {
        if self.coarse_pipeline.is_none() || self.fine.is_none() {
            return false;
        }
        let batch = WgpuCoarseBatch {
            draw_start,
            draw_end,
            layer_stack_start,
            layer_stack_end,
            active_tile_count: self.active_tiles.as_ref().map(DamageTiles::len),
        };
        self.coarse_pipeline.as_ref().unwrap().encode_in(
            commands,
            &self.scene_buffers,
            &self.scan,
            &mut self.coarse,
            self.lengths,
            batch,
        );
        self.fine_batch_to_in(commands, target)
    }

    fn fine_batch_to_in(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        let active_tile_count = self.active_tiles.as_ref().map(DamageTiles::len);
        if fine.uses_portable_textures() {
            return self.fine_portable_batch_to_in(commands, target);
        }
        match target {
            WgpuRenderTargetId::Main => {
                if let Some(target) = &self.root_target_view {
                    fine.render_tiles_to_view_in(
                        commands,
                        self.size.0,
                        self.size.1,
                        self.lengths,
                        &self.scene_buffers,
                        &self.scan,
                        &self.coarse,
                        &self.fine_spills,
                        &self.fine_indirect_args,
                        target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                        active_tile_count,
                    )
                } else {
                    fine.render_tiles_in(
                        commands,
                        self.size.0,
                        self.size.1,
                        self.lengths,
                        &self.scene_buffers,
                        &self.scan,
                        &self.coarse,
                        &self.fine_spills,
                        &self.fine_indirect_args,
                        &mut self.readback_target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                        active_tile_count,
                    )
                }
            }
            WgpuRenderTargetId::Scratch(ix) => fine.render_tiles_in(
                commands,
                self.size.0,
                self.size.1,
                self.lengths,
                &self.scene_buffers,
                &self.scan,
                &self.coarse,
                &self.fine_spills,
                &self.fine_indirect_args,
                &mut self.scratch[ix],
                self.clear_color,
                true,
                self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                active_tile_count,
            ),
        }
    }

    fn fine_portable_batch_to_in(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        let Some(target_texture) = self.render_target_texture(target).cloned() else {
            return false;
        };
        let active_tile_count = self.active_tiles.as_ref().map(DamageTiles::len);
        self.fine_portable_source
            .resize(commands.device(), self.size.0, self.size.1);
        self.fine_portable_target
            .resize(commands.device(), self.size.0, self.size.1);
        copy_texture(
            commands.encoder(),
            &target_texture,
            self.fine_portable_source.texture(),
            self.size,
        );
        let ok = fine.render_tiles_to_views_in(
            commands,
            self.size.0,
            self.size.1,
            self.lengths,
            &self.scene_buffers,
            &self.scan,
            &self.coarse,
            &self.fine_spills,
            &self.fine_indirect_args,
            self.fine_portable_source.view(),
            self.fine_portable_target.view(),
            self.clear_color,
            true,
            self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
            self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
            active_tile_count,
        );
        if ok {
            copy_texture(
                commands.encoder(),
                self.fine_portable_target.texture(),
                &target_texture,
                self.size,
            );
        }
        ok
    }

    fn execute_offscreen_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        draw: usize,
        layer: &Layer,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        match layer {
            Layer::Isolate => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                None,
                None,
                target,
                filter_cursors,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                Some(opacity.opacity),
                None,
                target,
                filter_cursors,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                None,
                Some(blend.mode),
                target,
                filter_cursors,
            ),
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer(
                commands,
                retained_id,
                canvas,
                plan,
                filter,
                sample_region,
                outer_stack,
                children,
                target,
                filter_cursors,
            ),
            Layer::Backdrop {
                filter,
                sample_region,
            } => self.execute_backdrop_layer(
                commands,
                retained_id,
                canvas,
                plan,
                filter,
                sample_region,
                outer_stack,
                children,
                target,
                filter_cursors,
            ),
            Layer::ClipSdf { .. } => false,
            Layer::Clip => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_masked_group_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        draw: usize,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        opacity: Option<f32>,
        blend: Option<peniko::BlendMode>,
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds = draw_bounds(canvas, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return true;
        }

        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Group,
                self.size,
                self.surface_origin,
                bounds,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_group(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                outer_stack,
                blend,
            );
            self.incremental_stats.reused_offscreen_surfaces += 1;
            self.retained_surfaces.insert(
                id,
                surface.meta,
                surface.primary,
                surface.secondary,
                surface.backdrop_source,
            );
            filter_cursors.advance_ops(children);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            self.incremental_stats.rerendered_offscreen_surfaces += 1;
            self.incremental_stats.rerendered_offscreen_tiles += if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
        }

        let source = if let Some((_, mut surface)) = cached {
            let Some(source) = self.acquire_scratch() else {
                return false;
            };
            self.install_scratch_render_target(source, surface.primary);
            if let Some(bounds) = self.active_region(bounds) {
                self.clear_render_region(commands, source, bounds, 0);
            }
            if !self.execute_ops(commands, canvas, plan, children, source, filter_cursors) {
                return false;
            }
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(source);
                return false;
            };
            self.install_scratch_render_target(
                mask,
                surface
                    .secondary
                    .take()
                    .expect("group cache has a retained mask"),
            );
            (source, Some(mask))
        } else {
            let Some(source) =
                self.render_ops_to_scratch(commands, canvas, plan, children, filter_cursors)
            else {
                return false;
            };
            (source, None)
        };
        let (source, cached_mask) = source;
        if let Some(opacity) = opacity
            && let Some(bounds) = self.active_region(bounds)
        {
            self.apply_color_filter_to_target(commands, source, bounds, FILTER_OPACITY, opacity);
        }

        let mask = if let Some(mask) = cached_mask {
            mask
        } else {
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(source);
                return false;
            };
            mask
        };
        if let Some(bounds) = self.active_region(bounds) {
            self.build_layer_mask(commands, mask, draw as u32, bounds);
        }
        let ok = self.composite_group_targets(
            commands,
            target,
            source,
            mask,
            bounds,
            outer_stack,
            blend,
        );
        if retained_id.is_some() && meta.is_some() {
            let source = self.take_scratch_target(source);
            let mask = self.take_scratch_target(mask);
            if let (Some(source), Some(mask)) = (source, mask) {
                self.cache_retained_surface(retained_id, meta, source, Some(mask), None);
            }
        } else {
            self.release_scratch(mask);
            self.release_scratch(source);
        }
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_filter_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        sample_region: &crate::shared::layer::region::Region,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let target_bounds = Bounds::canvas(self.size.0, self.size.1);
        let Some(filter_bounds) =
            filter_model::filter_surface_bounds(filter, sample_region, target_bounds)
        else {
            filter_cursors.advance_filter_layer(sample_region, children, filter);
            return true;
        };

        filter_cursors.advance_filter_layer(sample_region, children, filter);
        let surface_size = (
            filter_bounds.surface.width(),
            filter_bounds.surface.height(),
        );
        let surface_origin = (filter_bounds.surface.x0, filter_bounds.surface.y0);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Filter,
                surface_size,
                surface_origin,
                filter_bounds.output,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(filter_bounds.output)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_filter_surface(
                commands,
                target,
                &surface.primary,
                surface_size,
                surface_origin,
                filter_bounds.output,
                outer_stack,
            );
            self.incremental_stats.reused_offscreen_surfaces += 1;
            self.retained_surfaces.insert(
                id,
                surface.meta,
                surface.primary,
                surface.secondary,
                surface.backdrop_source,
            );
            return ok;
        }
        let local_damage = cached
            .as_ref()
            .filter(|(_, surface)| surface.secondary.is_some())
            .and_then(|_| self.local_damage_for_surface(filter_bounds.surface));
        let local = profile_cpu("prepare.local_scene", || {
            local_offscreen_scene(canvas, plan, children, filter_bounds.surface)
        });
        let local_filter = profile_cpu("prepare.local_filter", || {
            local_filter(filter, filter_bounds.surface)
        });
        let local_bounds = Bounds::canvas(
            filter_bounds.surface.width(),
            filter_bounds.surface.height(),
        );
        let local_origin = (
            self.surface_origin.0 + filter_bounds.surface.x0,
            self.surface_origin.1 + filter_bounds.surface.y0,
        );
        let cache_surface = retained_id.is_some() && meta.is_some();
        let local_scratch_count = if cache_surface {
            3 + required_scratch_count(&local.plan).max(filter_scratch_extra(&local_filter))
        } else {
            1 + required_scratch_count(&local.plan).max(filter_scratch_extra(&local_filter))
        };
        let saved = self.activate_local_scene_resources(
            &local.canvas,
            &local.plan,
            &local_filter,
            local_scratch_count,
            local_origin,
        );

        let source = WgpuRenderTargetId::Scratch(0);
        let filtered = WgpuRenderTargetId::Scratch(1);
        let partial_output =
            if let (Some((_, mut surface)), Some(output_damage)) = (cached, local_damage) {
                let source_history = surface
                    .secondary
                    .take()
                    .expect("partial filter cache has source history");
                self.install_scratch_target(0, source_history);
                self.install_scratch_target(1, surface.primary);
                let output_update = output_damage
                    .coalesced_rects(surface_size)
                    .into_iter()
                    .reduce(Bounds::union)
                    .unwrap_or(local_bounds);
                // Visible output damage can depend on source pixels outside the
                // root target (for example, a shape below the bottom edge blurred
                // back into view). Redraw the full source dependency window while
                // keeping the filtered write restricted to the visible output.
                self.active_tiles = Some(output_damage.outset(
                    surface_size,
                    filter_model::filter_dependency_outset(&local_filter),
                ));
                self.prepare_active_tile_buffers();
                self.clear_render_region(commands, source, local_bounds, 0);
                Some((output_update, output_damage))
            } else {
                self.scratch_in_use[0] = true;
                self.clear_render_target(commands, source, 0);
                None
            };
        if !self.scan_and_cumsum(commands, &local.canvas) {
            self.restore_root_scene_resources(saved);
            return false;
        }
        let mut local_filter_cursors = WgpuFilterCursors::default();
        let mut ok = self.execute_ops(
            commands,
            &local.canvas,
            &local.plan,
            &local.children,
            source,
            &mut local_filter_cursors,
        );
        let is_partial_output = partial_output.is_some();
        if let Some((output_update, output_damage)) = partial_output {
            let process_bounds = output_update
                .outset(filter_model::filter_dependency_outset(&local_filter))
                .intersect(local_bounds);
            let Some(temp) = self.acquire_scratch() else {
                self.restore_root_scene_resources(saved);
                return false;
            };
            ok = ok
                && self.copy_region_to_target(commands, source, temp, process_bounds)
                && self.apply_filter(
                    commands,
                    temp,
                    process_bounds,
                    &local_filter,
                    None,
                    &mut local_filter_cursors,
                );
            // The expanded source worklist includes every halo tile sampled by
            // the filter. Switch to the original output list before touching
            // retained filtered history so clean output tiles remain byte-for-
            // byte unchanged.
            self.active_tiles = Some(output_damage);
            self.prepare_filter_active_tile_work();
            ok = ok && self.copy_region_to_target(commands, temp, filtered, output_update);
            self.release_scratch(temp);
        } else {
            if cache_surface {
                self.scratch_in_use[1] = true;
                ok = ok && self.copy_region_to_target(commands, source, filtered, local_bounds);
            }
            ok = ok
                && self.apply_filter(
                    commands,
                    source,
                    local_bounds,
                    &local_filter,
                    None,
                    &mut local_filter_cursors,
                );
        }

        let rerendered_local_tiles = self
            .active_tiles
            .as_ref()
            .map_or_else(|| tile_count_for_bounds(local_bounds), DamageTiles::len);
        let (source_buffer, source_history) = if cache_surface {
            if is_partial_output {
                (
                    self.take_scratch_target(filtered).unwrap(),
                    Some(self.take_scratch_target(source).unwrap()),
                )
            } else {
                (
                    self.take_scratch_target(source).unwrap(),
                    Some(self.take_scratch_target(filtered).unwrap()),
                )
            }
        } else {
            let mut local_scratch = std::mem::take(&mut self.scratch);
            (local_scratch.remove(0), None)
        };
        self.scratch_in_use.clear();
        self.restore_root_scene_resources(saved);
        if retained_id.is_some() {
            self.incremental_stats.rerendered_offscreen_surfaces += 1;
            self.incremental_stats.rerendered_offscreen_tiles += rerendered_local_tiles;
        }
        let ok = ok
            && self.composite_cached_filter_surface(
                commands,
                target,
                &source_buffer,
                surface_size,
                surface_origin,
                filter_bounds.output,
                outer_stack,
            );
        self.cache_retained_surface(retained_id, meta, source_buffer, source_history, None);
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_backdrop_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        sample_region: &crate::shared::layer::region::Region,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds = filter_model::filtered_region_bounds(
            filter,
            sample_region,
            Bounds::canvas(self.size.0, self.size.1),
        );
        if bounds.is_empty() {
            filter_cursors.next_path_index(sample_region);
            return true;
        }
        let path_index = filter_cursors.next_path_index(sample_region);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Backdrop,
                self.size,
                self.surface_origin,
                bounds,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_backdrop(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                sample_region,
                outer_stack.clone(),
            );
            self.incremental_stats.reused_offscreen_surfaces += 1;
            self.retained_surfaces.insert(
                id,
                surface.meta,
                surface.primary,
                surface.secondary,
                surface.backdrop_source,
            );
            filter_cursors.advance_filter(filter);
            return ok
                && self.execute_ops(commands, canvas, plan, children, target, filter_cursors);
        }
        let partial = cached.as_ref().is_some_and(|(_, surface)| {
            surface.backdrop_source.is_some()
                && matches!(
                    filter,
                    Filter::Blur { sampling, .. } if sampling.factor() == 1
                )
        });
        if retained_id.is_some() {
            self.incremental_stats.rerendered_offscreen_surfaces += 1;
            self.incremental_stats.rerendered_offscreen_tiles += if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
        }

        if retained_id.is_none()
            && outer_stack.is_empty()
            && let Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } = filter
            && self.apply_downsampled_blur_rect_composite(
                commands,
                target,
                bounds,
                *std_dev_x,
                *std_dev_y,
                *sampling,
                sample_region,
            )
        {
            return self.execute_ops(commands, canvas, plan, children, target, filter_cursors);
        }

        if retained_id.is_none()
            && outer_stack.is_empty()
            && let Filter::RectLiquidGlass(glass) = filter
            && self.apply_downsampled_liquid_glass_rect_composite(
                commands,
                target,
                bounds,
                *glass,
                sample_region,
            )
        {
            return self.execute_ops(commands, canvas, plan, children, target, filter_cursors);
        }

        let cache_surface = retained_id.is_some() && meta.is_some();
        let (backdrop, cached_mask, cached_source) = if let Some((_, mut surface)) = cached {
            let Some(backdrop) = self.acquire_scratch() else {
                return false;
            };
            self.install_scratch_render_target(backdrop, surface.primary);
            (
                backdrop,
                surface.secondary.take(),
                surface.backdrop_source.take(),
            )
        } else {
            let Some(backdrop) = self.acquire_scratch() else {
                return false;
            };
            (backdrop, None, None)
        };

        // A retained root texture stores the final previous frame. Clean tiles
        // therefore contain this backdrop and any later foreground content,
        // not the painter-order input the filter must sample. Preserve that
        // pre-backdrop input separately and update it only from dirty tiles
        // after earlier draw operations have been replayed.
        let source_history = if cache_surface {
            let Some(source) = self.acquire_scratch() else {
                self.release_scratch(backdrop);
                return false;
            };
            let source_update = if cached_source.is_some() {
                self.active_region(bounds)
            } else {
                Some(bounds)
            };
            if let Some(cached_source) = cached_source {
                self.install_scratch_render_target(source, cached_source);
            }
            let source_ok = source_update
                .is_none_or(|bounds| self.copy_region_to_target(commands, target, source, bounds));
            if !source_ok {
                self.release_scratch(source);
                self.release_scratch(backdrop);
                return false;
            }
            Some(source)
        } else {
            None
        };
        let filter_source = source_history.unwrap_or(target);
        let filter_ok = if partial {
            let output = self
                .active_bounds_union()
                .unwrap_or(bounds)
                .intersect(bounds);
            let Filter::Blur {
                std_dev_x,
                std_dev_y,
                ..
            } = filter
            else {
                unreachable!("only full-resolution blur supports partial backdrop updates")
            };
            filter_cursors.advance_filter(filter);
            self.apply_blur_from_source_partial(
                commands,
                filter_source,
                backdrop,
                output,
                bounds,
                *std_dev_x,
                *std_dev_y,
            )
        } else {
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    sampling,
                } => self.apply_blur_from_source(
                    commands,
                    filter_source,
                    backdrop,
                    bounds,
                    *std_dev_x,
                    *std_dev_y,
                    *sampling,
                ),
                _ => {
                    self.copy_region_to_target(commands, filter_source, backdrop, bounds)
                        && self.apply_filter(
                            commands,
                            backdrop,
                            bounds,
                            filter,
                            Some(sample_region),
                            filter_cursors,
                        )
                }
            }
        };
        if !filter_ok {
            if let Some(source) = source_history {
                self.release_scratch(source);
            }
            self.release_scratch(backdrop);
            return false;
        }

        let mut retained_mask = None;
        let ok = if outer_stack.is_empty() {
            self.active_region(bounds).is_none_or(|bounds| {
                self.composite_src_over_rect_mask_direct(
                    commands,
                    target,
                    backdrop,
                    bounds,
                    sample_region,
                )
            })
        } else {
            let mask = if let Some(mask) = cached_mask {
                let Some(mask_target) = self.acquire_scratch() else {
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                };
                self.install_scratch_render_target(mask_target, mask);
                mask_target
            } else {
                let Some(mask) = self.acquire_scratch() else {
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                };
                if !self.build_region_mask(commands, mask, sample_region, path_index, bounds) {
                    self.release_scratch(mask);
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                }
                mask
            };

            let ok = self.active_region(bounds).is_none_or(|bounds| {
                self.composite_src_over_with_stack(
                    commands,
                    target,
                    backdrop,
                    Some(mask),
                    bounds,
                    outer_stack.clone(),
                )
            });
            retained_mask = Some(mask);
            ok
        };
        if cache_surface {
            let backdrop = self.take_scratch_target(backdrop);
            let mask = retained_mask.and_then(|mask| self.take_scratch_target(mask));
            let source = source_history.and_then(|source| self.take_scratch_target(source));
            if let (Some(backdrop), Some(source)) = (backdrop, source) {
                self.cache_retained_surface(retained_id, meta, backdrop, mask, Some(source));
            }
        } else {
            if let Some(mask) = retained_mask {
                self.release_scratch(mask);
            }
            if let Some(source) = source_history {
                self.release_scratch(source);
            }
            self.release_scratch(backdrop);
        }
        ok && self.execute_ops(commands, canvas, plan, children, target, filter_cursors)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_mask_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        layer: &crate::shared::layer::mask::Mask,
        outer_stack: std::ops::Range<usize>,
        content: &[ExecOp],
        mask_ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds =
            region_bounds(&layer.region).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            filter_cursors.next_path_index(&layer.region);
            return true;
        }
        let path_index = filter_cursors.next_path_index(&layer.region);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Mask,
                self.size,
                self.surface_origin,
                bounds,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_group(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                outer_stack,
                None,
            );
            self.incremental_stats.reused_offscreen_surfaces += 1;
            self.retained_surfaces.insert(
                id,
                surface.meta,
                surface.primary,
                surface.secondary,
                surface.backdrop_source,
            );
            filter_cursors.advance_ops(content);
            filter_cursors.advance_ops(mask_ops);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            self.incremental_stats.rerendered_offscreen_surfaces += 1;
            self.incremental_stats.rerendered_offscreen_tiles += if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
        }

        let (content_target, cached_mask) = if let Some((_, mut surface)) = cached {
            let Some(content_target) = self.acquire_scratch() else {
                return false;
            };
            self.install_scratch_render_target(content_target, surface.primary);
            if let Some(update) = self.active_region(bounds) {
                self.clear_render_region(commands, content_target, update, 0);
            }
            if !self.execute_ops(
                commands,
                canvas,
                plan,
                content,
                content_target,
                filter_cursors,
            ) {
                self.release_scratch(content_target);
                return false;
            }
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(content_target);
                return false;
            };
            self.install_scratch_render_target(
                mask,
                surface
                    .secondary
                    .take()
                    .expect("mask cache has retained mask coverage"),
            );
            (content_target, Some(mask))
        } else {
            let Some(content_target) =
                self.render_ops_to_scratch(commands, canvas, plan, content, filter_cursors)
            else {
                return false;
            };
            (content_target, None)
        };
        let Some(mask_source) =
            self.render_ops_to_scratch(commands, canvas, plan, mask_ops, filter_cursors)
        else {
            self.release_scratch(content_target);
            return false;
        };

        let mask = if let Some(mask) = cached_mask {
            mask
        } else {
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(mask_source);
                self.release_scratch(content_target);
                return false;
            };
            mask
        };
        if let Some(update) = self.active_region(bounds) {
            self.svg_mask_coverage(commands, mask_source, mask, update, layer.kind);
        }
        self.release_scratch(mask_source);

        let Some(region_mask) = self.acquire_scratch() else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        };
        let region_ok = self.active_region(bounds).is_none_or(|update| {
            self.build_region_mask(commands, region_mask, &layer.region, path_index, update) && {
                self.apply_region_mask(commands, region_mask, mask, update);
                true
            }
        });
        self.release_scratch(region_mask);
        if !region_ok {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        }

        let ok = self.composite_group_targets(
            commands,
            target,
            content_target,
            mask,
            bounds,
            outer_stack,
            None,
        );
        if retained_id.is_some() && meta.is_some() {
            let content = self.take_scratch_target(content_target);
            let mask = self.take_scratch_target(mask);
            if let (Some(content), Some(mask)) = (content, mask) {
                self.cache_retained_surface(retained_id, meta, content, Some(mask), None);
            }
        } else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
        }
        ok
    }

    fn render_ops_to_scratch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let target = self.acquire_scratch()?;
        self.clear_render_target(commands, target, 0);
        if self.execute_ops(commands, canvas, plan, ops, target, filter_cursors) {
            Some(target)
        } else {
            self.release_scratch(target);
            None
        }
    }

    fn build_layer_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        draw_ix: u32,
        bounds: Bounds,
    ) {
        if let Some(filter) = &self.filter {
            let bindings = self.scene_buffers.filter_bindings(&self.scan);
            filter.build_layer_mask(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                &bindings,
                draw_ix,
                bounds,
            );
        }
    }

    fn build_region_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        region: &crate::shared::layer::region::Region,
        path_index: Option<u32>,
        bounds: Bounds,
    ) -> bool {
        let paths = self.filter_path_bindings();
        self.filter.as_ref().is_some_and(|filter| {
            filter.build_region_mask(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                region,
                path_index,
                &paths,
                bounds,
            )
        })
    }

    fn svg_mask_coverage(
        &self,
        commands: &mut WgpuCommandBatch,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        kind: crate::shared::layer::mask::MaskKind,
    ) {
        if let Some(filter) = &self.filter {
            filter.svg_mask_coverage(
                commands,
                self.render_target_view(source),
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                kind,
            );
        }
    }

    fn apply_region_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        mask: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) {
        if let Some(filter) = &self.filter {
            let Some(target_read) = self.snapshot_filter_target(commands, target) else {
                return;
            };
            filter.apply_region_mask(
                commands,
                self.render_target_view(mask),
                self.render_target_view(target),
                target_read,
                self.size,
                self.lengths,
                bounds,
            );
        }
    }

    fn composite_src_over_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: Option<WgpuRenderTargetId>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target) else {
            return false;
        };
        filter.composite_src_over_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            mask.map(|mask| self.render_target_view(mask)),
            self.size,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
        );
        true
    }

    fn active_region(&self, bounds: Bounds) -> Option<Bounds> {
        match &self.active_tiles {
            Some(active) if active.intersects_bounds(bounds) => Some(bounds),
            Some(_) => None,
            None => Some(bounds),
        }
    }

    fn active_tile_count(&self, bounds: Bounds) -> u32 {
        self.active_tiles.as_ref().map_or_else(
            || tile_count_for_bounds(bounds),
            |tiles| tiles.count_in_bounds(bounds),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_group_targets(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: WgpuRenderTargetId,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> bool {
        self.active_region(bounds).is_none_or(|bounds| {
            if let Some(mode) = blend {
                self.composite_blend_with_stack(
                    commands,
                    target,
                    source,
                    mask,
                    bounds,
                    layer_stack.clone(),
                    mode,
                )
            } else {
                self.composite_src_over_with_stack(
                    commands,
                    target,
                    source,
                    Some(mask),
                    bounds,
                    layer_stack.clone(),
                )
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_cached_group(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        mask: Option<&WgpuTarget>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(mask) = mask else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        self.active_region(bounds).is_none_or(|bounds| {
            let Some(target_read) = self.snapshot_filter_target(commands, target) else {
                return false;
            };
            if let Some(mode) = blend {
                filter.composite_blend_with_stack(
                    commands,
                    self.render_target_view(target),
                    target_read,
                    source.view(),
                    mask.view(),
                    self.size,
                    self.lengths,
                    &bindings,
                    bounds,
                    layer_stack.clone(),
                    mode,
                );
            } else {
                filter.composite_src_over_with_stack(
                    commands,
                    self.render_target_view(target),
                    target_read,
                    source.view(),
                    Some(mask.view()),
                    self.size,
                    self.lengths,
                    &bindings,
                    bounds,
                    layer_stack.clone(),
                );
            }
            true
        })
    }

    fn composite_src_over_rect_mask_direct(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        bounds: Bounds,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(target_read) = self.snapshot_filter_target(commands, target) else {
            return false;
        };
        filter.composite_src_over_rect_mask_direct(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            self.size,
            self.lengths,
            bounds,
            region,
        )
    }

    fn composite_cached_backdrop(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        mask: Option<&WgpuTarget>,
        bounds: Bounds,
        region: &crate::shared::layer::region::Region,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        if !layer_stack.is_empty() {
            return self.composite_cached_group(
                commands,
                target,
                source,
                mask,
                bounds,
                layer_stack,
                None,
            );
        }
        let Some(filter) = &self.filter else {
            return false;
        };
        self.active_region(bounds).is_none_or(|bounds| {
            let Some(target_read) = self.snapshot_filter_target(commands, target) else {
                return false;
            };
            filter.composite_src_over_rect_mask_direct(
                commands,
                self.render_target_view(target),
                target_read,
                source.view(),
                self.size,
                self.lengths,
                bounds,
                region,
            )
        })
    }

    fn composite_blend_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: WgpuRenderTargetId,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        mode: peniko::BlendMode,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target) else {
            return false;
        };
        filter.composite_blend_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            self.render_target_view(mask),
            self.size,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
            mode,
        );
        true
    }

    fn composite_surface_src_over_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target) else {
            return false;
        };
        filter.composite_src_over_surface_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            source.view(),
            self.size,
            source_size,
            source_origin,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
        );
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_cached_filter_surface(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        self.active_region(bounds).is_none_or(|bounds| {
            self.composite_surface_src_over_with_stack(
                commands,
                target,
                source,
                source_size,
                source_origin,
                bounds,
                layer_stack.clone(),
            )
        })
    }

    fn acquire_scratch(&mut self) -> Option<WgpuRenderTargetId> {
        for (ix, in_use) in self.scratch_in_use.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return Some(WgpuRenderTargetId::Scratch(ix));
            }
        }
        None
    }

    fn release_scratch(&mut self, target: WgpuRenderTargetId) {
        let WgpuRenderTargetId::Scratch(ix) = target else {
            return;
        };
        self.scratch_in_use[ix] = false;
    }

    fn take_scratch_target(&mut self, target: WgpuRenderTargetId) -> Option<WgpuTarget> {
        let WgpuRenderTargetId::Scratch(ix) = target else {
            return None;
        };
        let replacement = WgpuTarget::new(&self.device, self.size.0, self.size.1);
        self.scratch_in_use[ix] = false;
        Some(std::mem::replace(&mut self.scratch[ix], replacement))
    }

    fn install_scratch_target(&mut self, index: usize, target: WgpuTarget) {
        debug_assert_eq!(target.size(), self.size);
        self.scratch[index] = target;
        self.scratch_in_use[index] = true;
    }

    fn install_scratch_render_target(&mut self, target_id: WgpuRenderTargetId, target: WgpuTarget) {
        let WgpuRenderTargetId::Scratch(index) = target_id else {
            unreachable!("retained surfaces can only occupy scratch targets")
        };
        self.install_scratch_target(index, target);
    }

    fn active_bounds_union(&self) -> Option<Bounds> {
        self.active_tiles
            .as_ref()?
            .coalesced_rects(self.size)
            .into_iter()
            .reduce(Bounds::union)
    }

    fn render_target_view(&self, target: WgpuRenderTargetId) -> &::wgpu::TextureView {
        match target {
            WgpuRenderTargetId::Main => self
                .root_target_view
                .as_ref()
                .unwrap_or(self.readback_target.view()),
            WgpuRenderTargetId::Scratch(ix) => self.scratch[ix].view(),
        }
    }

    fn render_target_texture(&self, target: WgpuRenderTargetId) -> Option<&::wgpu::Texture> {
        match target {
            WgpuRenderTargetId::Main => Some(
                self.root_target_texture
                    .as_ref()
                    .unwrap_or(self.readback_target.texture()),
            ),
            WgpuRenderTargetId::Scratch(ix) => Some(self.scratch.get(ix)?.texture()),
        }
    }

    fn snapshot_filter_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
    ) -> Option<&::wgpu::TextureView> {
        copy_texture(
            commands.encoder(),
            self.render_target_texture(target)?,
            self.filter_target_snapshot.texture(),
            self.size,
        );
        Some(self.filter_target_snapshot.view())
    }

    fn filter_brush_bindings(&self) -> WgpuFilterBrushBindings<'_> {
        let image_resources = self.scene_buffers.image_resource_bindings();
        WgpuFilterBrushBindings {
            blob: self.filter_brushes.blob.buffer(),
            image_resource_atlas: image_resources.atlas,
            image_resource_sampler: image_resources.sampler,
            image_resource_texture_views: image_resources.texture_views,
            image_resource_dummy_texture: image_resources.dummy_texture,
        }
    }

    fn filter_turbulence_bindings(&self) -> WgpuFilterTurbulenceBindings<'_> {
        WgpuFilterTurbulenceBindings {
            selectors: self.filter_turbulence.selectors.buffer(),
            gradients: self.filter_turbulence.gradients.buffer(),
        }
    }

    fn filter_path_bindings(&self) -> WgpuFilterPathBindings<'_> {
        WgpuFilterPathBindings {
            range_starts: self.filter_paths.range_starts.buffer(),
            range_ends: self.filter_paths.range_ends.buffer(),
            p0x: self.filter_paths.p0x.buffer(),
            p0y: self.filter_paths.p0y.buffer(),
            p1x: self.filter_paths.p1x.buffer(),
            p1y: self.filter_paths.p1y.buffer(),
        }
    }

    pub fn render_with_text(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        assert!(
            self.render_native_with_text(canvas, font_system, text_context),
            "wgpu renderer could not render text scene natively"
        );
    }

    pub fn render_with_options(
        &mut self,
        canvas: &Canvas,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        let mode = self.incremental_config.mode;
        self.incremental_config.mode = super::incremental::IncrementalRenderMode::ForceFull;
        let selected = self.select_scene(canvas);
        let frame = selected.frame();
        let retained_ptr = selected.retained_ptr();
        let scene = selected.scene();
        let plan = self.begin_incremental_frame(frame, scene);
        self.prepare_scene(scene);
        self.mark_scene_prepared(retained_ptr, false);
        let rendered_native = self.render_prepared_tile_plan(scene);
        self.finish_incremental_frame(plan, rendered_native);
        self.incremental_config.mode = mode;
        if rendered_native {
            self.size = scene.physical_size();
            let image = self.image();
            let debug = self.read_debug_scan_buffers();
            return capture_render_debug(
                "wgpu",
                scene,
                &image,
                DebugScanBuffers {
                    backdrops: &debug.backdrops,
                    tile_segment_ranges: &debug.tile_segment_ranges,
                    segments: &debug.segments,
                },
                options,
            );
        }

        panic!("wgpu renderer could not render debug scene natively")
    }

    fn read_debug_scan_buffers(&self) -> WgpuDebugScanReadback {
        let backdrops =
            self.scan
                .backdrops
                .read::<i32>(&self.device, &self.queue, self.lengths.backdrop_len);
        let tile_segment_ranges = self.scan.tile_segment_ranges.read::<TileSegmentRange>(
            &self.device,
            &self.queue,
            self.lengths.backdrop_len,
        );
        let segments = self.scan.segments.read::<LineSegment>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        WgpuDebugScanReadback {
            backdrops,
            tile_segment_ranges,
            segments,
        }
    }

    pub fn image(&self) -> Image {
        let byte_len = self.target_rgba8_byte_len();
        if byte_len == 0 {
            return Image {
                width: self.size.0,
                height: self.size.1,
                pixels: Vec::new(),
            };
        }

        let row_bytes = self.target_row_bytes();
        let padded_row_bytes =
            row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
        let readback = self.device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu target readback"),
            size: padded_row_bytes * self.size.1 as u64,
            usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&::wgpu::CommandEncoderDescriptor {
                label: Some("tileink wgpu target readback copy"),
            });
        encoder.copy_texture_to_buffer(
            self.readback_target.texture().as_image_copy(),
            ::wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: ::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes as u32),
                    rows_per_image: None,
                },
            },
            self.target_texture_extent(),
        );
        self.queue.submit([encoder.finish()]);

        let (tx, rx) = mpsc::channel();
        readback
            .slice(..)
            .map_async(::wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap()
            });
        self.device
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("poll wgpu device for target readback");
        rx.recv()
            .expect("receive target readback map result")
            .expect("map wgpu target readback buffer");

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .expect("read mapped wgpu target readback buffer");
        let mut pixels = Vec::with_capacity(self.size.0 as usize * self.size.1 as usize);
        for row in 0..self.size.1 as usize {
            let start = row * padded_row_bytes as usize;
            let row = &mapped[start..start + row_bytes as usize];
            pixels.extend_from_slice(bytemuck::cast_slice(row));
        }
        drop(mapped);
        readback.unmap();
        Image {
            width: self.size.0,
            height: self.size.1,
            pixels,
        }
    }

    pub fn device(&self) -> &::wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &::wgpu::Queue {
        &self.queue
    }

    pub fn target_rgba8_byte_len(&self) -> ::wgpu::BufferAddress {
        rgba8_byte_len(self.size.0, self.size.1)
    }

    pub fn render_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture(canvas, dst)?;
        self.last_frame_used_native = true;
        Ok(())
    }

    pub fn render_with_text_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_with_text_to_wgpu_texture(canvas, font_system, text_context, dst)?;
        self.last_frame_used_native = true;
        Ok(())
    }

    fn render_native_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture_with_prepare(canvas, dst, false, |renderer, canvas| {
            renderer.prepare_scene(canvas);
        })
    }

    fn render_native_with_text_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture_with_prepare(canvas, dst, true, |renderer, canvas| {
            renderer.prepare_scene_with_text(canvas, font_system, text_context);
        })
    }

    fn render_native_to_wgpu_texture_with_prepare(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
        uses_text: bool,
        prepare: impl FnOnce(&mut Self, &Canvas),
    ) -> Result<(), WgpuTextureRenderError> {
        self.validate_wgpu_storage_texture_destination(
            dst,
            canvas.physical_width(),
            canvas.physical_height(),
        )?;
        let selected = self.select_scene(canvas);
        let frame = selected.frame();
        let retained_ptr = selected.retained_ptr();
        let is_retained = frame.is_some();
        if is_retained && !dst.usage().contains(::wgpu::TextureUsages::COPY_DST) {
            return Err(WgpuTextureRenderError::DestinationUsageMissing(dst.usage()));
        }
        if !is_retained {
            self.root_target_texture = Some(dst.clone());
            self.root_target_view =
                Some(dst.create_view(&::wgpu::TextureViewDescriptor::default()));
        }
        let scene = selected.scene();
        let plan = self.begin_incremental_frame(frame, scene);
        let has_work = !plan.tiles.is_empty();
        if has_work && self.scene_needs_prepare(retained_ptr, uses_text) {
            prepare(self, scene);
            self.mark_scene_prepared(retained_ptr, uses_text);
        }
        let rendered = !has_work || self.render_prepared_tile_plan(scene);
        self.root_target_view = None;
        self.root_target_texture = None;
        self.finish_incremental_frame(plan, rendered);
        if rendered {
            self.size = scene.physical_size();
            if is_retained {
                self.copy_history_to(dst);
            }
            return Ok(());
        }
        panic!("wgpu renderer could not render scene natively")
    }

    fn copy_history_to(&self, dst: &::wgpu::Texture) {
        let mut encoder = self
            .device
            .create_command_encoder(&::wgpu::CommandEncoderDescriptor {
                label: Some("tileink retained history copy"),
            });
        copy_texture(&mut encoder, self.readback_target.texture(), dst, self.size);
        self.queue.submit([encoder.finish()]);
    }

    fn validate_wgpu_storage_texture_destination(
        &self,
        dst: &::wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuTextureRenderError> {
        if dst.width() < width || dst.height() < height {
            return Err(WgpuTextureRenderError::DestinationTooSmall {
                required_width: width,
                required_height: height,
                actual_width: dst.width(),
                actual_height: dst.height(),
            });
        }
        if !dst.usage().contains(::wgpu::TextureUsages::STORAGE_BINDING) {
            return Err(WgpuTextureRenderError::DestinationStorageUsageMissing(
                dst.usage(),
            ));
        }
        if self
            .fine
            .as_ref()
            .is_some_and(WgpuFinePipeline::uses_portable_textures)
            && !dst
                .usage()
                .contains(::wgpu::TextureUsages::COPY_SRC | ::wgpu::TextureUsages::COPY_DST)
        {
            return Err(WgpuTextureRenderError::DestinationUsageMissing(dst.usage()));
        }
        if dst.format() != ::wgpu::TextureFormat::Rgba8Unorm
            || dst.dimension() != ::wgpu::TextureDimension::D2
            || dst.sample_count() != 1
        {
            return Err(WgpuTextureRenderError::UnsupportedDestination {
                format: dst.format(),
                dimension: dst.dimension(),
                sample_count: dst.sample_count(),
            });
        }
        Ok(())
    }

    fn target_row_bytes(&self) -> ::wgpu::BufferAddress {
        self.size.0 as ::wgpu::BufferAddress * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
    }

    fn target_texture_extent(&self) -> ::wgpu::Extent3d {
        ::wgpu::Extent3d {
            width: self.size.0,
            height: self.size.1,
            depth_or_array_layers: 1,
        }
    }
}

struct WgpuDebugScanReadback {
    backdrops: Vec<i32>,
    tile_segment_ranges: Vec<TileSegmentRange>,
    segments: Vec<LineSegment>,
}

fn rgba8_byte_len(width: u32, height: u32) -> ::wgpu::BufferAddress {
    width as ::wgpu::BufferAddress
        * height as ::wgpu::BufferAddress
        * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
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
