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
    ActiveScanPlan, CoarseBinningMode, DamageTiles, IncrementalRenderConfig, IncrementalRenderStats,
};
use super::lazy::PipelineCompilationTracker;
use super::profile::{WgpuRenderProfile, WgpuRenderProfiler, profile_cpu, start_cpu_scope};
use super::retained_surfaces::{RetainedSurface, RetainedSurfaceKind, RetainedSurfaceMeta};
use super::scan::WgpuScanPipeline;
use super::target::WgpuTarget;

mod filter_ops;
mod output;
mod retained;
mod scene;

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
    clear_color: u32,
    profiler: WgpuRenderProfiler,
    last_frame_used_native: bool,
    size: (u32, u32),
    surface_origin: (i32, i32),
    persistent_scene: Option<PersistentSceneMaterializer>,
    persistent_scene_rendered: Option<(u64, SceneVersion)>,
}

struct SavedRendererState {
    lengths: GpuBufferLengths,
    plan: Option<Rc<ExecPlan>>,
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
    scratch_spares: Vec<WgpuTarget>,
    scratch_in_use: Vec<bool>,
    size: (u32, u32),
    surface_origin: (i32, i32),
    active_tiles: Option<DamageTiles>,
    filter_active_tile_work: Option<FilterTileWork>,
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
        let range_scatter_pipeline = Rc::new(WgpuRangeScatterPipeline::new(
            device,
            pipeline_cache,
            &pipeline_compilations,
        ));
        Self {
            device: device.clone(),
            queue: queue.clone(),
            lengths: GpuBufferLengths::default(),
            plan: None,
            config: WgpuBuffer::new(device, "tileink wgpu canvas config"),
            scene_buffers: WgpuSceneBuffers::new(device, range_scatter_pipeline.clone()),
            range_scatter_pipeline,
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
            prepared_plan_fingerprint: None,
            retained: RetainedRenderState::new(IncrementalRenderConfig::default()),
            filter_tile_work_arena: FilterTileWorkArena::default(),
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
            scratch_spares: Vec::new(),
            scratch_in_use: Vec::new(),
            clear_color: premul_clear_color(clear),
            profiler: WgpuRenderProfiler::default(),
            last_frame_used_native: true,
            size: (width, height),
            surface_origin: (0, 0),
            persistent_scene: None,
            persistent_scene_rendered: None,
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
        assert!(
            self.render_native(canvas),
            "wgpu renderer could not render scene natively"
        );
    }

    pub fn render_retained(&mut self, scene: &RetainedScene) {
        let (canvas, reused) = self.retained_scene_canvas(scene);
        let selected =
            self.retained
                .select_materialized(canvas, reused, scene.id(), scene.version());
        assert!(
            self.render_native_selected(selected, false, None),
            "wgpu renderer could not render retained scene natively"
        );
        self.mark_retained_scene_rendered(scene);
    }

    pub fn render_retained_with_text(
        &mut self,
        scene: &RetainedScene,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        let (canvas, reused) = self.retained_scene_canvas(scene);
        let selected =
            self.retained
                .select_materialized(canvas, reused, scene.id(), scene.version());
        assert!(
            self.render_native_selected(selected, true, Some((font_system, text_context)),),
            "wgpu renderer could not render retained text scene natively"
        );
        self.mark_retained_scene_rendered(scene);
    }

    pub fn render_retained_profiled(&mut self, scene: &RetainedScene) -> WgpuRenderProfile {
        self.start_profile();
        self.render_retained(scene);
        self.end_profile().clone()
    }

    pub fn render_retained_with_text_profiled(
        &mut self,
        scene: &RetainedScene,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) -> WgpuRenderProfile {
        self.start_profile();
        self.render_retained_with_text(scene, font_system, text_context);
        self.end_profile().clone()
    }

    pub(crate) fn retained_scene_canvas(&mut self, scene: &RetainedScene) -> (Rc<Canvas>, bool) {
        let id = scene.id();
        let switched = self
            .persistent_scene
            .as_ref()
            .is_some_and(|cached| cached.scene_id() != id);
        if switched {
            self.retained.invalidate();
            self.persistent_scene_rendered = None;
        }
        if self.persistent_scene.is_none() || switched {
            self.persistent_scene = Some(profile_cpu("retained.materialize", || {
                PersistentSceneMaterializer::new(scene)
            }));
        }
        let materializer = self.persistent_scene.as_mut().unwrap();
        let unchanged = materializer.version() == scene.version();
        let plan_may_change = !unchanged
            && scene
                .changes_since(materializer.version())
                .is_none_or(|changes| changes.topology_changed || changes.surface_changed);
        if plan_may_change {
            // GPU submission no longer borrows the previous frame's plan. Releasing that Rc
            // lets the materializer patch layer fragments in place instead of cloning an
            // otherwise scene-sized ExecPlan for one changed layer.
            self.plan = None;
        }
        let scene_data_changed = profile_cpu("retained.materialize", || materializer.update(scene));
        if materializer.version() != scene.version() {
            unreachable!("persistent materializer did not consume scene version");
        }
        if scene_data_changed {
            self.retained.invalidate_prepared_scene();
        }
        let canvas = materializer.canvas();
        if plan_may_change {
            self.plan = canvas.compiled_plan.clone();
        }
        (canvas, unchanged || !scene_data_changed)
    }

    pub(crate) fn mark_retained_scene_rendered(&mut self, scene: &RetainedScene) {
        self.persistent_scene_rendered = Some((scene.id(), scene.version()));
    }

    pub fn insert_image(&mut self, key: ImageKey, image: impl Into<Rc<Image>>) -> bool {
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
        self.profiler
            .end(&self.device, &self.queue, self.retained.stats().clone())
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
        self.retained.config()
    }

    pub fn set_incremental_render_config(&mut self, config: IncrementalRenderConfig) {
        self.retained.set_config(config);
    }

    pub fn incremental_render_stats(&self) -> &IncrementalRenderStats {
        self.retained.stats()
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
        self.retained.invalidate();
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
        self.retained.set_history_owner(HistoryOwner::Internal);
        self.retained.reset_transient_output();
        let selected = SelectedScene::Borrowed(canvas);
        self.render_native_selected(selected, false, None)
    }

    fn render_native_selected(
        &mut self,
        selected: SelectedScene<'_>,
        uses_text: bool,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) -> bool {
        let frame = selected.frame();
        let materialization = selected.materialization();
        let materialized_reused = selected.materialized_reused();
        let scene = selected.scene();
        let plan = self
            .retained
            .begin_frame(frame, scene, self.profiler.is_active());
        self.retained.stats_mut().materialized_scene_reused = materialized_reused;
        let has_work = !plan.tiles.is_empty();
        // A structural commit can add no immediate pixel damage (for example, an empty retained
        // layer) while still changing the persistent plan and GPU tables needed by later commits.
        // Consume that state now; only raster execution remains conditional on `has_work`.
        if self
            .retained
            .scene_needs_prepare(materialization, uses_text, self.image_resources_dirty)
        {
            if let Some((font_system, text_context)) = text {
                self.prepare_scene_with_text(scene, font_system, text_context);
            } else {
                self.prepare_scene(scene);
            }
            self.retained
                .mark_scene_prepared(materialization, uses_text);
        }
        let rendered = !has_work || self.render_prepared_native(scene);
        self.retained.finish_frame(plan, rendered, true);
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
        self.retained.set_history_owner(HistoryOwner::Internal);
        self.retained.reset_transient_output();
        let selected = SelectedScene::Borrowed(canvas);
        let frame = selected.frame();
        let materialization = selected.materialization();
        let materialized_reused = selected.materialized_reused();
        let scene = selected.scene();
        let plan = self
            .retained
            .begin_frame(frame, scene, self.profiler.is_active());
        self.retained.stats_mut().materialized_scene_reused = materialized_reused;
        let has_work = !plan.tiles.is_empty();
        // Keep plan/resource cursors current even when this commit has no raster work.
        if self
            .retained
            .scene_needs_prepare(materialization, true, self.image_resources_dirty)
        {
            self.prepare_scene_with_text(scene, font_system, text_context);
            self.retained.mark_scene_prepared(materialization, true);
        }
        let rendered = !has_work || self.render_prepared_native(scene);
        self.retained.finish_frame(plan, rendered, true);
        rendered
    }

    fn retained_surface_meta(
        &self,
        id: crate::canvas::RetainedSurfaceId,
        kind: RetainedSurfaceKind,
        size: (u32, u32),
        origin: (i32, i32),
        bounds: Bounds,
    ) -> Option<RetainedSurfaceMeta> {
        self.retained.surface_meta(id, kind, size, origin, bounds)
    }

    fn retained_surface_is_dirty(&self, bounds: Bounds) -> bool {
        self.retained.surface_is_dirty(bounds)
    }

    fn local_damage_for_surface(&self, surface: Bounds) -> Option<DamageTiles> {
        self.retained.local_damage_for_surface(surface, self.size)
    }

    fn prepare_active_tile_buffers(&mut self) {
        if let Some(active) = self.retained.active_tiles() {
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
            .retained
            .active_tiles()
            .map(|active| active.list().to_vec())
        else {
            if let Some(filter) = self.filter.as_mut() {
                filter.clear_active_tile_work();
            }
            return;
        };
        self.prepare_filter_tile_work(&tiles);
    }

    fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles> {
        let active = self.retained.take_active_tiles();
        if active.is_some()
            && let Some(filter) = self.filter.as_mut()
        {
            filter.clear_active_tile_work();
        }
        active
    }

    fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>) {
        self.retained.set_active_tiles(active);
        self.prepare_filter_active_tile_work();
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
        self.retained.take_matching_surface(id, meta)
    }

    fn cache_retained_surface(
        &mut self,
        id: Option<crate::canvas::RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
        primary: WgpuTarget,
        secondary: Option<WgpuTarget>,
        backdrop_source: Option<WgpuTarget>,
    ) {
        self.retained
            .cache_surface(id, meta, primary, secondary, backdrop_source);
    }

    fn scan_and_cumsum(&mut self, commands: &mut WgpuCommandBatch, scene: &Canvas) -> bool {
        let (Some(scan), Some(cumsum)) = (&self.scan_pipeline, &self.cumsum) else {
            return false;
        };
        self.coarse
            .encode_pending_tile_bin_copies(commands.encoder());
        let active = self
            .retained
            .active_tiles()
            .map(|damage| ActiveScanPlan::new(scene, damage, self.scene_upload.scan_ranges()));
        let stats = self.retained.stats_mut();
        if let Some(active) = &active {
            self.scan
                .upload_active_indices(&self.device, &self.queue, &active.indices);
            self.scan
                .upload_active_cumsum_plan(&self.device, &self.queue, &active.cumsum);
            stats.scanned_paths += active.path_count;
            stats.scanned_lines += active.line_count;
            stats.scan_chunks += active.chunk_count;
        } else {
            stats.scanned_paths += self.lengths.path_count as u32;
            stats.scanned_lines += self.lengths.line_count as u32;
            stats.scan_chunks += self.lengths.scan_chunk_count as u32;
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
        self.render_prepared_tile_plan_with_history_copy(canvas, None)
    }

    fn render_prepared_tile_plan_with_history_copy(
        &mut self,
        canvas: &Canvas,
        history_copy_dst: Option<&::wgpu::Texture>,
    ) -> bool {
        if self
            .retained
            .active_tiles()
            .is_some_and(DamageTiles::is_empty)
        {
            if let Some(dst) = history_copy_dst {
                let mut commands =
                    WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
                self.encode_history_copy(&mut commands, dst);
                self.retained.stats_mut().queue_submissions = commands.finish();
            }
            return true;
        }
        if self.fine.is_none() || self.coarse_pipeline.is_none() || self.filter.is_none() {
            return false;
        }
        let Some(plan) = self.plan.clone() else {
            return false;
        };

        if let Some(filter) = &self.filter {
            filter.reset_dispatch_counts();
        }
        self.filter_tile_work_arena.reset();
        self.prepare_active_tile_buffers();

        let mut commands = WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
        if !self.scan_and_cumsum(&mut commands, canvas) {
            self.retained.stats_mut().queue_submissions = commands.finish();
            return false;
        }
        if self.retained.active_tiles().is_some() {
            self.clear_render_region(
                &mut commands,
                WgpuRenderTargetId::Main,
                Bounds::canvas(self.size.0, self.size.1),
                self.clear_color,
            );
        } else {
            self.clear_render_target(&mut commands, WgpuRenderTargetId::Main, self.clear_color);
        }
        let draw_batch_ids = canvas
            .stable_batch_ids
            .as_deref()
            .unwrap_or(&plan.draw_batch_ids);
        let active_batches = profile_cpu("plan.active_batches", || {
            self.retained.active_tiles().map(|tiles| {
                self.scene_upload
                    .active_batch_ids(tiles.list(), draw_batch_ids)
            })
        });
        let mut filter_cursors = WgpuFilterCursors::default();
        let ok = profile_cpu("plan.execute", || {
            let direct_ops = active_batches
                .as_ref()
                .and_then(|active| plan.active_direct_root_ops(active));
            if let Some(ops) = direct_ops {
                self.execute_direct_root_batches(&mut commands, canvas, &plan, &ops)
            } else {
                self.execute_ops(
                    &mut commands,
                    canvas,
                    &plan,
                    &plan.ops,
                    WgpuRenderTargetId::Main,
                    &mut filter_cursors,
                    active_batches.as_deref(),
                )
            }
        });
        if ok && let Some(dst) = history_copy_dst {
            self.encode_history_copy(&mut commands, dst);
        }
        self.retained.stats_mut().queue_submissions = commands.finish();
        if let Some(filter) = &self.filter {
            let (dispatches, compact_dispatches) = filter.dispatch_counts();
            let stats = self.retained.stats_mut();
            stats.filter_dispatches = dispatches;
            stats.compact_filter_dispatches = compact_dispatches;
        }
        ok
    }

    fn encode_history_copy(&self, commands: &mut WgpuCommandBatch, dst: &::wgpu::Texture) {
        let _profile_scope = start_cpu_scope("history.copy");
        copy_texture(
            commands.encoder(),
            self.readback_target.texture(),
            dst,
            self.size,
        );
    }

    fn execute_ops(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
        active_batches: Option<&[u32]>,
    ) -> bool {
        for op in ops {
            let ok = match op {
                ExecOp::DrawBatch {
                    draws,
                    batch_id,
                    layer_stack,
                    ..
                } => {
                    active_batches.is_some_and(|active| active.binary_search(batch_id).is_err())
                        || self.execute_draw_batch(
                            commands,
                            canvas,
                            draws,
                            *batch_id,
                            layer_stack.clone(),
                            target,
                        )
                }
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
        scene: &Canvas,
        draws: &[usize],
        batch_id: u32,
        layer_stack: std::ops::Range<usize>,
        target: WgpuRenderTargetId,
    ) -> bool {
        let live = scene
            .stable_batch_counts
            .as_ref()
            .map_or(!draws.is_empty(), |counts| {
                counts.get(batch_id as usize).copied().unwrap_or(0) != 0
            });
        if !live {
            return true;
        }
        let stats = self.retained.stats_mut();
        stats.draw_batches = stats.draw_batches.saturating_add(1);
        if target == WgpuRenderTargetId::Main {
            stats.root_draw_batches = stats.root_draw_batches.saturating_add(1);
        }
        profile_cpu("plan.draw_batch", || {
            self.coarse_and_fine_batch_to(
                commands,
                batch_id,
                batch_id.saturating_add(1),
                layer_stack.start as u32,
                layer_stack.end as u32,
                target,
            )
        })
    }

    fn execute_direct_root_batches(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> bool {
        for &index in ops {
            let ExecOp::DrawBatch {
                draws,
                batch_id,
                layer_stack,
                ..
            } = &plan.ops[index]
            else {
                unreachable!("direct root index points to a draw batch")
            };
            if !self.execute_draw_batch(
                commands,
                canvas,
                draws,
                *batch_id,
                layer_stack.clone(),
                WgpuRenderTargetId::Main,
            ) {
                return false;
            }
        }
        true
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
        let binning_stats = self
            .retained
            .active_tiles()
            .map(|active| self.scene_upload.coarse_binning_stats(active.list()));
        let active_tile_count = binning_stats.map(|stats| stats.active_tiles);
        let dense =
            binning_stats.is_some_and(|stats| match self.retained.config().coarse_binning {
                CoarseBinningMode::Auto => prefer_dense_binning(self.lengths, stats),
                CoarseBinningMode::ForceCompact => false,
                CoarseBinningMode::ForceDense => stats.active_tiles != 0,
            });
        if dense {
            self.retained.stats_mut().dense_coarse_batches += 1;
        }
        let batch = WgpuCoarseBatch {
            draw_start,
            draw_end,
            layer_stack_start,
            layer_stack_end,
            active_tile_count: if dense { None } else { active_tile_count },
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
        let active_tile_count = self.retained.active_tiles().map(DamageTiles::len);
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
        let active_tile_count = self.retained.active_tiles().map(DamageTiles::len);
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

        let (meta, mut cached) = {
            let _scope = start_cpu_scope("plan.group.cache");
            let meta = retained_id.and_then(|id| {
                self.retained_surface_meta(
                    id,
                    RetainedSurfaceKind::Group,
                    self.size,
                    self.surface_origin,
                    bounds,
                )
            });
            let cached = self.take_matching_retained_surface(retained_id, meta);
            (meta, cached)
        };
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = {
                let _scope = start_cpu_scope("plan.group.composite");
                self.composite_cached_group(
                    commands,
                    target,
                    &surface.primary,
                    surface.secondary.as_ref(),
                    bounds,
                    outer_stack,
                    blend,
                )
            };
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_ops(children);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
        }

        let source = {
            let _scope = start_cpu_scope("plan.group.children");
            if let Some((_, mut surface)) = cached {
                let source = {
                    let _scope = start_cpu_scope("plan.group.scratch");
                    let Some(source) = self.acquire_scratch() else {
                        return false;
                    };
                    self.install_scratch_render_target(source, surface.primary);
                    source
                };
                if let Some(bounds) = self.active_region(bounds) {
                    self.clear_render_region(commands, source, bounds, 0);
                }
                let rendered = profile_cpu("plan.group.render", || {
                    self.execute_ops(
                        commands,
                        canvas,
                        plan,
                        children,
                        source,
                        filter_cursors,
                        None,
                    )
                });
                if !rendered {
                    return false;
                }
                let mask = {
                    let _scope = start_cpu_scope("plan.group.scratch");
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
                    mask
                };
                (source, Some(mask))
            } else {
                let source = profile_cpu("plan.group.render", || {
                    self.render_ops_to_scratch(commands, canvas, plan, children, filter_cursors)
                });
                let Some(source) = source else {
                    return false;
                };
                (source, None)
            }
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
            let _scope = start_cpu_scope("plan.group.mask");
            self.build_layer_mask(commands, mask, draw as u32, bounds);
        }
        let ok = {
            let _scope = start_cpu_scope("plan.group.composite");
            self.composite_group_targets(commands, target, source, mask, bounds, outer_stack, blend)
        };
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

        let root_filter_cursors = filter_cursors.clone();
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
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            return ok;
        }
        let local_damage = cached
            .as_ref()
            .filter(|(_, surface)| surface.secondary.is_some())
            .and_then(|_| self.local_damage_for_surface(filter_bounds.surface));
        // A full-canvas filter already uses the local surface coordinate space. Borrowing its
        // immutable scene and plan avoids cloning/translating every retained arena for one dirty
        // allocation; cropped or offset surfaces still take the general translation path.
        let translated_local = (filter_bounds.surface
            != Bounds::canvas(canvas.physical_width(), canvas.physical_height()))
        .then(|| {
            let candidate_draws = self
                .scene_upload
                .draws_in_bounds(filter_bounds.surface, plan);
            profile_cpu("prepare.local_scene", || {
                local_offscreen_scene(
                    canvas,
                    plan,
                    children,
                    filter_bounds.surface,
                    &candidate_draws,
                )
            })
        });
        let (local_canvas, local_plan, local_children) = translated_local
            .as_ref()
            .map_or((canvas, plan, children), |local| {
                (&local.canvas, &local.plan, local.children.as_slice())
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
            3 + required_scratch_count(local_plan).max(filter_scratch_extra(&local_filter))
        } else {
            1 + required_scratch_count(local_plan).max(filter_scratch_extra(&local_filter))
        };
        let reuse_root_resources = translated_local.is_none()
            && target == WgpuRenderTargetId::Main
            && self.surface_origin == local_origin;
        let mut saved = if reuse_root_resources {
            self.prepare_scratch_buffers(local_scratch_count.max(1));
            None
        } else {
            Some(self.activate_local_scene_resources(
                local_canvas,
                local_plan,
                &local_filter,
                local_scratch_count,
                local_origin,
            ))
        };

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
                self.retained.set_active_tiles(Some(output_damage.outset(
                    surface_size,
                    filter_model::filter_dependency_outset(&local_filter),
                )));
                self.prepare_active_tile_buffers();
                self.clear_render_region(commands, source, local_bounds, 0);
                Some((output_update, output_damage))
            } else {
                self.scratch_in_use[0] = true;
                self.clear_render_target(commands, source, 0);
                None
            };
        if !self.scan_and_cumsum(commands, local_canvas) {
            if let Some(saved) = saved.take() {
                self.restore_root_scene_resources(saved);
            }
            return false;
        }
        let mut local_filter_cursors = if reuse_root_resources {
            root_filter_cursors
        } else {
            WgpuFilterCursors::default()
        };
        let mut ok = self.execute_ops(
            commands,
            local_canvas,
            local_plan,
            local_children,
            source,
            &mut local_filter_cursors,
            None,
        );
        let is_partial_output = partial_output.is_some();
        if let Some((output_update, output_damage)) = partial_output {
            let process_bounds = output_update
                .outset(filter_model::filter_dependency_outset(&local_filter))
                .intersect(local_bounds);
            let Some(temp) = self.acquire_scratch() else {
                if let Some(saved) = saved.take() {
                    self.restore_root_scene_resources(saved);
                }
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
            self.retained.set_active_tiles(Some(output_damage));
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
            .retained
            .active_tiles()
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
        if let Some(saved) = saved {
            self.scratch_in_use.clear();
            self.restore_root_scene_resources(saved);
        } else {
            self.scratch_in_use.fill(false);
        }
        if retained_id.is_some() {
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_local_tiles;
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
        let backdrop_dirty = self.retained.backdrop_is_dirty(retained_id);
        if !backdrop_dirty && let Some((id, surface)) = cached.take() {
            let ok = self.composite_cached_backdrop(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                sample_region,
                outer_stack.clone(),
            );
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_filter(filter);
            return ok
                && self.execute_ops(
                    commands,
                    canvas,
                    plan,
                    children,
                    target,
                    filter_cursors,
                    None,
                );
        }
        let partial = cached.as_ref().is_some_and(|(_, surface)| {
            surface.backdrop_source.is_some()
                && (matches!(filter, Filter::Blur { sampling, .. } if sampling.factor() == 1)
                    || matches!(
                        filter,
                        Filter::RectLiquidGlass(glass) if glass.blur_sampling.factor() == 1
                    ))
        });
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
        }

        let direct_backdrop = retained_id.is_none() || self.retained.bypasses_backdrop_cache();
        if direct_backdrop
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
            return self.execute_ops(
                commands,
                canvas,
                plan,
                children,
                target,
                filter_cursors,
                None,
            );
        }

        if direct_backdrop
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
            return self.execute_ops(
                commands,
                canvas,
                plan,
                children,
                target,
                filter_cursors,
                None,
            );
        }

        // A full-root redraw cannot reuse this frame's temporary source/output history. Keeping
        // it would add a target->history copy to every backdrop (and can double liquid-glass
        // cost under an outer clip) merely to discard or invalidate it on the next moving frame.
        let cache_surface =
            retained_id.is_some() && meta.is_some() && !self.retained.bypasses_backdrop_cache();
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
        // Filters without a partial-update implementation rebuild their whole cached surface.
        // Blur needs explicit intermediate halos. Liquid glass is safe with the root worklist
        // because its cached source is complete and retained damage already includes the full
        // blur/refraction dependency outset needed by every dirty output tile.
        let suspended_active = (!partial).then(|| self.suspend_incremental_filter_work());
        let filter_ok = if partial {
            let output = self
                .active_bounds_union()
                .unwrap_or(bounds)
                .intersect(bounds);
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    ..
                } => {
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
                }
                Filter::RectLiquidGlass(glass) => {
                    rect_liquid_glass_region(Some(sample_region), bounds).is_some_and(|region| {
                        self.apply_liquid_glass_from_source_partial(
                            commands,
                            filter_source,
                            backdrop,
                            output,
                            bounds,
                            *glass,
                            region,
                        )
                    })
                }
                _ => unreachable!("only blur and liquid glass support partial backdrop updates"),
            }
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
        if let Some(active) = suspended_active {
            self.restore_incremental_filter_work(active);
        }
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
        ok && self.execute_ops(
            commands,
            canvas,
            plan,
            children,
            target,
            filter_cursors,
            None,
        )
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
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_ops(content);
            filter_cursors.advance_ops(mask_ops);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
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
                None,
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
        if self.execute_ops(commands, canvas, plan, ops, target, filter_cursors, None) {
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
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        match self.retained.active_tiles() {
            Some(active) if active.intersects_bounds(bounds) => Some(bounds),
            Some(_) => None,
            None => Some(bounds),
        }
    }

    fn active_tile_count(&self, bounds: Bounds) -> u32 {
        self.retained.active_tiles().map_or_else(
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
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
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
        // A retained surface temporarily owns the texture that occupied this slot. Reuse the
        // displaced slot target on the inverse transfer instead of allocating and immediately
        // dropping a full-size placeholder texture every incremental frame.
        let replacement = self
            .scratch_spares
            .pop()
            .unwrap_or_else(|| WgpuTarget::new(&self.device, self.size.0, self.size.1));
        self.scratch_in_use[ix] = false;
        Some(std::mem::replace(&mut self.scratch[ix], replacement))
    }

    fn install_scratch_target(&mut self, index: usize, target: WgpuTarget) {
        debug_assert_eq!(target.size(), self.size);
        let displaced = std::mem::replace(&mut self.scratch[index], target);
        self.scratch_spares.push(displaced);
        self.scratch_in_use[index] = true;
    }

    fn install_scratch_render_target(&mut self, target_id: WgpuRenderTargetId, target: WgpuTarget) {
        let WgpuRenderTargetId::Scratch(index) = target_id else {
            unreachable!("retained surfaces can only occupy scratch targets")
        };
        self.install_scratch_target(index, target);
    }

    fn active_bounds_union(&self) -> Option<Bounds> {
        self.retained
            .active_tiles()?
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
        bounds: Bounds,
    ) -> Option<&::wgpu::TextureView> {
        copy_texture_region(
            commands.encoder(),
            self.render_target_texture(target)?,
            self.filter_target_snapshot.texture(),
            self.size,
            bounds,
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
        self.retained.set_history_owner(HistoryOwner::Internal);
        self.retained.reset_transient_output();
        let mode = self
            .retained
            .replace_mode(super::incremental::IncrementalRenderMode::ForceFull);
        let selected = SelectedScene::Borrowed(canvas);
        let frame = selected.frame();
        let materialization = selected.materialization();
        let materialized_reused = selected.materialized_reused();
        let scene = selected.scene();
        let plan = self
            .retained
            .begin_frame(frame, scene, self.profiler.is_active());
        self.retained.stats_mut().materialized_scene_reused = materialized_reused;
        self.prepare_scene(scene);
        self.retained.mark_scene_prepared(materialization, false);
        let rendered_native = self.render_prepared_tile_plan(scene);
        self.retained.finish_frame(plan, rendered_native, true);
        self.retained.replace_mode(mode);
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
