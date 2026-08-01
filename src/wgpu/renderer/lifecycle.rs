//! Renderer construction, public configuration, and retained-scene lifecycle.

use super::*;

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
            local_scene_resource_pool: Vec::new(),
            pending_local_scene_resources: Vec::new(),
            #[cfg(feature = "bench-internals")]
            reuse_local_scene_resources: true,
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
        let changes = scene.changes_since(materializer.version());
        let plan_may_change = !unchanged
            && changes
                .as_ref()
                .is_none_or(|changes| changes.topology_changed || changes.surface_changed);
        if plan_may_change {
            // GPU submission no longer borrows the previous frame's plan. Releasing that Rc
            // lets the materializer patch layer fragments in place instead of cloning an
            // otherwise scene-sized ExecPlan for one changed layer.
            self.plan = None;
        }
        let scene_data_changed = profile_cpu("retained.materialize", || {
            materializer.update(scene, changes)
        });
        assert_eq!(
            materializer.version(),
            scene.version(),
            "persistent materializer did not consume scene version"
        );
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

    /// Number of initialized compute pipelines backed by embedded DXIL.
    ///
    /// This is observable so applications and GPU regressions can distinguish the startup cache
    /// from the output-equivalent runtime WGSL fallback instead of inferring the source from time.
    pub fn precompiled_dxil_pipeline_count(&self) -> u64 {
        self.pipeline_compilations.precompiled_dxil_count()
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

    pub(super) fn render_native_selected(
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

    pub(super) fn retained_surface_meta(
        &self,
        id: crate::canvas::RetainedSurfaceId,
        kind: RetainedSurfaceKind,
        size: (u32, u32),
        origin: (i32, i32),
        bounds: Bounds,
    ) -> Option<RetainedSurfaceMeta> {
        self.retained.surface_meta(id, kind, size, origin, bounds)
    }

    pub(super) fn retained_surface_is_dirty(&self, bounds: Bounds) -> bool {
        self.retained.surface_is_dirty(bounds)
    }

    pub(super) fn local_damage_for_surface(&self, surface: Bounds) -> Option<DamageTiles> {
        self.retained.local_damage_for_surface(surface, self.size)
    }

    pub(super) fn prepare_active_tile_buffers(&mut self) {
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
    pub(super) fn prepare_filter_active_tile_work(&mut self) {
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

    pub(super) fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles> {
        let active = self.retained.take_active_tiles();
        if active.is_some()
            && let Some(filter) = self.filter.as_mut()
        {
            filter.clear_active_tile_work();
        }
        active
    }

    pub(super) fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>) {
        self.retained.set_active_tiles(active);
        self.prepare_filter_active_tile_work();
    }

    pub(super) fn prepare_filter_tile_work(&mut self, tiles: &[u32]) {
        let work = self
            .filter_tile_work_arena
            .upload(&self.device, &self.queue, tiles);
        if let Some(filter) = self.filter.as_mut() {
            filter.restore_active_tile_work(Some(work));
        }
    }

    pub(super) fn take_matching_retained_surface(
        &mut self,
        id: Option<crate::canvas::RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
    ) -> Option<(crate::canvas::RetainedSurfaceId, RetainedSurface)> {
        self.retained.take_matching_surface(id, meta)
    }

    pub(super) fn cache_retained_surface(
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
}
