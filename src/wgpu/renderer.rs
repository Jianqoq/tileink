#![allow(clippy::too_many_arguments)]

use std::sync::mpsc;

use peniko::Color;

use crate::{
    cpu::Renderer as CpuRenderer,
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    canvas::Canvas,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, ROOT_COMMAND_LIST_ID},
        gpu_plan::{
            FINE_GROUP_SPILL_FIELDS, FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH,
            FINE_WORKGROUP_SIZE, GpuBufferLengths, GpuSceneConfig, filter_scratch_extra,
            plan_stack_depths, required_scratch_count,
        },
        image::Image,
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
use super::fine::{WgpuFinePipeline, premul_clear_color};
use super::profile::{WgpuRenderProfile, WgpuRenderProfiler, profile_cpu, start_cpu_scope};
use super::scan::WgpuScanPipeline;
use super::scene::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuSceneUploadStaging};
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
pub struct Renderer {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    cpu: CpuRenderer,
    lengths: GpuBufferLengths,
    plan: Option<ExecPlan>,
    config: WgpuBuffer,
    scene_buffers: WgpuSceneBuffers,
    scene_upload: WgpuSceneUploadStaging,
    scan: WgpuScanBuffers,
    coarse: WgpuCoarseBuffers,
    max_clip_depth: usize,
    max_group_depth: usize,
    fine_clip_spills: WgpuBuffer,
    fine_group_spills: WgpuBuffer,
    text_data: Option<PreparedTextData>,
    scan_pipeline: Option<WgpuScanPipeline>,
    cumsum: Option<WgpuCumsumPipeline>,
    coarse_pipeline: Option<WgpuCoarsePipeline>,
    fine: Option<WgpuFinePipeline>,
    filter: Option<WgpuFilterPipeline>,
    filter_transfers: WgpuFilterTransferBuffers,
    filter_brushes: WgpuFilterBrushBuffers,
    filter_convolves: WgpuFilterConvolveBuffers,
    filter_turbulence: WgpuFilterTurbulenceBuffers,
    filter_paths: WgpuFilterPathBuffers,
    // Compatibility surface for render()/image(); render_to_wgpu_texture writes caller-owned textures directly.
    readback_target: WgpuTarget,
    root_target_view: Option<::wgpu::TextureView>,
    scratch: Vec<WgpuTarget>,
    scratch_in_use: Vec<bool>,
    clear_color: u32,
    profiler: WgpuRenderProfiler,
    size: (u32, u32),
    surface_origin: (i32, i32),
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
    fine_clip_spills: WgpuBuffer,
    fine_group_spills: WgpuBuffer,
    filter_transfers: WgpuFilterTransferBuffers,
    filter_brushes: WgpuFilterBrushBuffers,
    filter_convolves: WgpuFilterConvolveBuffers,
    filter_turbulence: WgpuFilterTurbulenceBuffers,
    filter_paths: WgpuFilterPathBuffers,
    readback_target: WgpuTarget,
    root_target_view: Option<::wgpu::TextureView>,
    scratch: Vec<WgpuTarget>,
    scratch_in_use: Vec<bool>,
    size: (u32, u32),
    surface_origin: (i32, i32),
}

impl Render for Renderer {
    type ScanArgs<'a> = ();
    type CumsumArgs<'a> = ();
    type CoarseArgs<'a> = WgpuCoarseBatch;
    type ExecuteArgs<'a> = ();

    fn render(&mut self, scene: &Canvas) {
        if self.render_native(scene) {
            return;
        }
        profile_cpu("cpu_fallback.render", || self.cpu.render(scene));
        self.upload_cpu_image();
    }

    fn execute(&mut self, scene: &Canvas, _: Self::ExecuteArgs<'_>) {
        self.render(scene);
    }

    fn scan(&mut self, scene: &Canvas, _: Self::ScanArgs<'_>) {
        if let Some(scan) = &self.scan_pipeline {
            scan.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        } else {
            profile_cpu("cpu_fallback.scan", || {
                <CpuRenderer as Render>::scan(&mut self.cpu, scene, ())
            });
        }
    }

    fn cumsum(&mut self, scene: &Canvas, _: Self::CumsumArgs<'_>) {
        if let Some(cumsum) = &self.cumsum {
            cumsum.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        } else {
            profile_cpu("cpu_fallback.cumsum", || {
                <CpuRenderer as Render>::cumsum(&mut self.cpu, scene, ())
            });
        }
    }

    fn coarse(&mut self, _: &Canvas, batch: Self::CoarseArgs<'_>) {
        if let Some(coarse) = &self.coarse_pipeline {
            coarse.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &self.scan,
                &mut self.coarse,
                self.lengths,
                batch,
            );
        }
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
        Self {
            device: device.clone(),
            queue: queue.clone(),
            cpu: CpuRenderer::new(width, height, clear),
            lengths: GpuBufferLengths::default(),
            plan: None,
            config: WgpuBuffer::new(device, "tileink wgpu scene config"),
            scene_buffers: WgpuSceneBuffers::new(device),
            scene_upload: WgpuSceneUploadStaging::default(),
            scan: WgpuScanBuffers::new(device),
            coarse: WgpuCoarseBuffers::new(device),
            max_clip_depth: 0,
            max_group_depth: 0,
            fine_clip_spills: WgpuBuffer::new(device, "tileink wgpu fine clip spills"),
            fine_group_spills: WgpuBuffer::new(device, "tileink wgpu fine group spills"),
            text_data: None,
            scan_pipeline: WgpuScanPipeline::new(device),
            cumsum: WgpuCumsumPipeline::new(device),
            coarse_pipeline: WgpuCoarsePipeline::new(device),
            fine: WgpuFinePipeline::new(device),
            filter: WgpuFilterPipeline::new(device),
            filter_transfers: WgpuFilterTransferBuffers::new(device),
            filter_brushes: WgpuFilterBrushBuffers::new(device),
            filter_convolves: WgpuFilterConvolveBuffers::new(device),
            filter_turbulence: WgpuFilterTurbulenceBuffers::new(device),
            filter_paths: WgpuFilterPathBuffers::new(device),
            readback_target: WgpuTarget::new(device, width, height),
            root_target_view: None,
            scratch: Vec::new(),
            scratch_in_use: Vec::new(),
            clear_color: premul_clear_color(clear),
            profiler: WgpuRenderProfiler::default(),
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
            }))
            .expect("request default wgpu adapter");
        let required_features = adapter.features()
            & (::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | ::wgpu::Features::TIMESTAMP_QUERY);
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

    pub fn render(&mut self, scene: &Canvas) {
        <Self as Render>::render(self, scene);
    }

    pub fn render_profiled(&mut self, scene: &Canvas) -> WgpuRenderProfile {
        self.start_profile();
        self.render(scene);
        self.end_profile().clone()
    }

    pub fn render_with_text_profiled(
        &mut self,
        scene: &Canvas,
        text_context: &mut TextContext,
    ) -> WgpuRenderProfile {
        self.start_profile();
        self.render_with_text(scene, text_context);
        self.end_profile().clone()
    }

    /// Starts collecting CPU stage timings and GPU pass timestamps.
    ///
    /// GPU durations require a device created with `wgpu::Features::TIMESTAMP_QUERY`.
    /// `new_default_device` requests that feature when the adapter supports it.
    pub fn start_profile(&mut self) {
        self.profiler.start(&self.device);
    }

    /// Stops profiling, waits for pending timestamp readback, and returns the latest profile.
    pub fn end_profile(&mut self) -> &WgpuRenderProfile {
        self.profiler.end(&self.device, &self.queue)
    }

    pub fn profile(&self) -> &WgpuRenderProfile {
        self.profiler.profile()
    }

    /// Updates the clear color without rebuilding device-owned pipelines, so one renderer can
    /// render multiple scenes/examples in a single process.
    pub fn set_clear_color(&mut self, clear: Color) {
        self.clear_color = premul_clear_color(clear);
        self.cpu.set_clear_color(clear);
    }

    /// Renders only through native wgpu compute pipelines.
    ///
    /// This is useful for backend parity tests because `render` falls back to the
    /// CPU renderer when a scene still needs unsupported native coverage.
    pub fn render_native(&mut self, scene: &Canvas) -> bool {
        self.prepare_scene(scene);
        self.render_prepared_native(scene)
    }

    /// Renders text scenes only through native wgpu compute pipelines.
    ///
    /// The caller supplies the text context so backend parity tests can compare
    /// native wgpu against another backend using exactly the same glyph atlas.
    pub fn render_native_with_text(
        &mut self,
        scene: &Canvas,
        text_context: &mut TextContext,
    ) -> bool {
        self.prepare_scene_with_text(scene, text_context);
        self.render_prepared_native(scene)
    }

    fn render_prepared_native(&mut self, scene: &Canvas) -> bool {
        if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            return true;
        }
        false
    }

    fn prepare_scene(&mut self, scene: &Canvas) {
        let _profile_scope = start_cpu_scope("prepare");
        self.text_data = None;
        self.prepare_scene_resources(scene);
    }

    fn prepare_scene_with_text(&mut self, scene: &Canvas, text_context: &mut TextContext) {
        let _profile_scope = start_cpu_scope("prepare");
        self.text_data = profile_cpu("prepare.text", || {
            Some(PreparedTextData::new(
                &scene.text_glyphs,
                &scene.text_runs,
                text_context,
            ))
        });
        self.prepare_scene_resources(scene);
    }

    fn prepare_scene_resources(&mut self, scene: &Canvas) {
        self.size = (scene.width, scene.height);
        self.surface_origin = (0, 0);
        profile_cpu("prepare.target", || {
            if self.root_target_view.is_none() {
                self.readback_target
                    .resize(&self.device, scene.width, scene.height);
            }
        });
        let lengths = profile_cpu("prepare.lengths", || {
            GpuBufferLengths::from_scene_with_text(scene, self.text_data.as_ref())
        });
        let plan = profile_cpu("prepare.compile", || scene.compile(ROOT_COMMAND_LIST_ID));
        let (max_clip_depth, max_group_depth) =
            profile_cpu("prepare.stack_depths", || plan_stack_depths(&plan));
        profile_cpu("prepare.upload_scene", || {
            self.scene_buffers.upload(
                &self.device,
                &self.queue,
                scene,
                &plan,
                self.text_data.as_ref(),
                &mut self.scene_upload,
            );
        });
        profile_cpu("prepare.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.coarse_buffers", || {
            self.coarse.prepare_outputs(&self.device, lengths);
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
            self.filter_brushes.upload(&self.device, &self.queue, &plan);
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
                "tileink wgpu scene config",
                &[GpuSceneConfig::new(scene, lengths, self.clear_color)],
            );
        });
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan);
    }

    fn activate_local_scene_resources(
        &mut self,
        scene: &Canvas,
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
                WgpuBuffer::new(&self.device, "tileink wgpu scene config"),
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
            fine_clip_spills: std::mem::replace(
                &mut self.fine_clip_spills,
                WgpuBuffer::new(&self.device, "tileink wgpu fine clip spills"),
            ),
            fine_group_spills: std::mem::replace(
                &mut self.fine_group_spills,
                WgpuBuffer::new(&self.device, "tileink wgpu fine group spills"),
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
                WgpuTarget::new(&self.device, scene.width, scene.height),
            ),
            root_target_view: std::mem::take(&mut self.root_target_view),
            scratch: std::mem::take(&mut self.scratch),
            scratch_in_use: std::mem::take(&mut self.scratch_in_use),
            size: self.size,
            surface_origin: self.surface_origin,
        };

        let lengths = profile_cpu("prepare.local.lengths", || {
            GpuBufferLengths::from_scene_with_text(scene, self.text_data.as_ref())
        });
        let (max_clip_depth, max_group_depth) =
            profile_cpu("prepare.local.stack_depths", || plan_stack_depths(plan));
        self.size = (scene.width, scene.height);
        self.surface_origin = surface_origin;
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan.clone());
        profile_cpu("prepare.local.upload_scene", || {
            self.scene_buffers.upload(
                &self.device,
                &self.queue,
                scene,
                plan,
                self.text_data.as_ref(),
                &mut self.scene_upload,
            );
        });
        profile_cpu("prepare.local.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.local.coarse_buffers", || {
            self.coarse.prepare_outputs(&self.device, lengths);
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
                "tileink wgpu scene config",
                &[GpuSceneConfig::new(scene, lengths, self.clear_color)],
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
        self.fine_clip_spills = saved.fine_clip_spills;
        self.fine_group_spills = saved.fine_group_spills;
        self.filter_transfers = saved.filter_transfers;
        self.filter_brushes = saved.filter_brushes;
        self.filter_convolves = saved.filter_convolves;
        self.filter_turbulence = saved.filter_turbulence;
        self.filter_paths = saved.filter_paths;
        self.readback_target = saved.readback_target;
        self.root_target_view = saved.root_target_view;
        self.scratch = saved.scratch;
        self.scratch_in_use = saved.scratch_in_use;
        self.size = saved.size;
        self.surface_origin = saved.surface_origin;
    }

    fn prepare_scratch_buffers(&mut self, count: usize) {
        while self.scratch.len() < count {
            self.scratch
                .push(WgpuTarget::new(&self.device, self.size.0, self.size.1));
        }
        for scratch in &mut self.scratch {
            scratch.resize(&self.device, self.size.0, self.size.1);
        }
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
        self.fine_clip_spills.resize_uninit::<u32>(
            &self.device,
            "tileink wgpu fine clip spills",
            lane_count * clip_spill_depth,
        );
        self.fine_group_spills.resize_uninit::<u32>(
            &self.device,
            "tileink wgpu fine group spills",
            lane_count * group_spill_depth * FINE_GROUP_SPILL_FIELDS,
        );
    }

    fn scan_and_cumsum(&mut self, commands: &mut WgpuCommandBatch, _scene: &Canvas) -> bool {
        let (Some(scan), Some(cumsum)) = (&self.scan_pipeline, &self.cumsum) else {
            return false;
        };
        scan.run_in(commands, &self.scene_buffers, &mut self.scan, self.lengths);
        cumsum.run_in(commands, &self.scene_buffers, &mut self.scan, self.lengths);
        true
    }

    #[cfg(test)]
    fn coarse_batch(
        &mut self,
        scene: &Canvas,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
    ) {
        <Self as Render>::coarse(
            self,
            scene,
            WgpuCoarseBatch {
                draw_start,
                draw_end,
                layer_stack_start,
                layer_stack_end,
            },
        );
    }

    fn render_prepared_tile_plan(&mut self, scene: &Canvas) -> bool {
        if self.fine.is_none() || self.coarse_pipeline.is_none() || self.filter.is_none() {
            return false;
        }
        let Some(plan) = self.plan.clone() else {
            return false;
        };

        let mut commands = WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
        if !self.scan_and_cumsum(&mut commands, scene) {
            return false;
        }
        self.clear_render_target(&mut commands, WgpuRenderTargetId::Main, self.clear_color);
        let mut filter_cursors = WgpuFilterCursors::default();
        let ok = self.execute_ops(
            &mut commands,
            scene,
            &plan,
            &plan.ops,
            WgpuRenderTargetId::Main,
            &mut filter_cursors,
        );
        commands.finish();
        ok
    }

    fn execute_ops(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        for op in ops {
            let ok = match op {
                ExecOp::DrawBatch { draws, layer_stack } => self.execute_draw_batch(
                    commands,
                    scene,
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
                    draw,
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    commands,
                    scene,
                    plan,
                    *draw,
                    layer,
                    outer_stack.clone(),
                    children,
                    target,
                    filter_cursors,
                ),
                ExecOp::OffscreenMaskLayer {
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => self.execute_mask_layer(
                    commands,
                    scene,
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
                        &self.fine_clip_spills,
                        &self.fine_group_spills,
                        target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
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
                        &self.fine_clip_spills,
                        &self.fine_group_spills,
                        &mut self.readback_target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
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
                &self.fine_clip_spills,
                &self.fine_group_spills,
                &mut self.scratch[ix],
                self.clear_color,
                true,
                self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
            ),
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
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
                scene,
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
                scene,
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
                scene,
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
                scene,
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
                scene,
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
        scene: &Canvas,
        plan: &ExecPlan,
        draw: usize,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        opacity: Option<f32>,
        blend: Option<peniko::BlendMode>,
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds = draw_bounds(scene, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return true;
        }

        let Some(source) =
            self.render_ops_to_scratch(commands, scene, plan, children, filter_cursors)
        else {
            return false;
        };
        if let Some(opacity) = opacity {
            self.apply_color_filter_to_target(commands, source, bounds, FILTER_OPACITY, opacity);
        }

        let Some(mask) = self.acquire_scratch() else {
            self.release_scratch(source);
            return false;
        };
        self.build_layer_mask(commands, mask, draw as u32, bounds);
        let ok = if let Some(mode) = blend {
            self.composite_blend_with_stack(
                commands,
                target,
                source,
                mask,
                bounds,
                outer_stack,
                mode,
            )
        } else {
            self.composite_src_over_with_stack(
                commands,
                target,
                source,
                Some(mask),
                bounds,
                outer_stack,
            )
        };
        self.release_scratch(mask);
        self.release_scratch(source);
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_filter_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
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
        let local = profile_cpu("prepare.local_scene", || {
            local_offscreen_scene(scene, plan, children, filter_bounds.surface)
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
        let local_scratch_count =
            1 + required_scratch_count(&local.plan).max(filter_scratch_extra(&local_filter));
        let saved = self.activate_local_scene_resources(
            &local.scene,
            &local.plan,
            &local_filter,
            local_scratch_count,
            local_origin,
        );

        let source = WgpuRenderTargetId::Scratch(0);
        self.scratch_in_use[0] = true;
        self.clear_render_target(commands, source, 0);
        if !self.scan_and_cumsum(commands, &local.scene) {
            self.restore_root_scene_resources(saved);
            return false;
        }
        let mut local_filter_cursors = WgpuFilterCursors::default();
        let ok = self.execute_ops(
            commands,
            &local.scene,
            &local.plan,
            &local.children,
            source,
            &mut local_filter_cursors,
        ) && self.apply_filter(
            commands,
            source,
            local_bounds,
            &local_filter,
            None,
            &mut local_filter_cursors,
        );

        let mut local_scratch = std::mem::take(&mut self.scratch);
        let source_buffer = local_scratch.remove(0);
        self.scratch_in_use.clear();
        self.restore_root_scene_resources(saved);
        ok && self.composite_surface_src_over_with_stack(
            commands,
            target,
            &source_buffer,
            (
                filter_bounds.surface.width(),
                filter_bounds.surface.height(),
            ),
            (filter_bounds.surface.x0, filter_bounds.surface.y0),
            filter_bounds.output,
            outer_stack,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_backdrop_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
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

        if outer_stack.is_empty()
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
            return self.execute_ops(commands, scene, plan, children, target, filter_cursors);
        }

        if outer_stack.is_empty()
            && let Filter::RectLiquidGlass(glass) = filter
            && self.apply_downsampled_liquid_glass_rect_composite(
                commands,
                target,
                bounds,
                *glass,
                sample_region,
            )
        {
            return self.execute_ops(commands, scene, plan, children, target, filter_cursors);
        }

        let Some(backdrop) = self.acquire_scratch() else {
            return false;
        };
        let filter_ok = match filter {
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => self.apply_blur_from_source(
                commands, target, backdrop, bounds, *std_dev_x, *std_dev_y, *sampling,
            ),
            _ => {
                self.copy_region_to_target(commands, target, backdrop, bounds)
                    && self.apply_filter(
                        commands,
                        backdrop,
                        bounds,
                        filter,
                        Some(sample_region),
                        filter_cursors,
                    )
            }
        };
        if !filter_ok {
            self.release_scratch(backdrop);
            return false;
        }

        let ok = if outer_stack.is_empty() {
            self.composite_src_over_rect_mask_direct(
                commands,
                target,
                backdrop,
                bounds,
                sample_region,
            )
        } else {
            false
        };
        let ok = if ok {
            true
        } else {
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(backdrop);
                return false;
            };
            let mask_ok = self.build_region_mask(commands, mask, sample_region, path_index, bounds);
            if !mask_ok {
                self.release_scratch(mask);
                self.release_scratch(backdrop);
                return false;
            }

            let ok = self.composite_src_over_with_stack(
                commands,
                target,
                backdrop,
                Some(mask),
                bounds,
                outer_stack,
            );
            self.release_scratch(mask);
            ok
        };
        self.release_scratch(backdrop);
        ok && self.execute_ops(commands, scene, plan, children, target, filter_cursors)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_mask_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
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

        let Some(content_target) =
            self.render_ops_to_scratch(commands, scene, plan, content, filter_cursors)
        else {
            return false;
        };
        let Some(mask_source) =
            self.render_ops_to_scratch(commands, scene, plan, mask_ops, filter_cursors)
        else {
            self.release_scratch(content_target);
            return false;
        };

        let Some(mask) = self.acquire_scratch() else {
            self.release_scratch(mask_source);
            self.release_scratch(content_target);
            return false;
        };
        self.svg_mask_coverage(commands, mask_source, mask, bounds, layer.kind);
        self.release_scratch(mask_source);

        let Some(region_mask) = self.acquire_scratch() else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        };
        let region_ok =
            self.build_region_mask(commands, region_mask, &layer.region, path_index, bounds);
        if region_ok {
            self.apply_region_mask(commands, region_mask, mask, bounds);
        }
        self.release_scratch(region_mask);
        if !region_ok {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        }

        let ok = self.composite_src_over_with_stack(
            commands,
            target,
            content_target,
            Some(mask),
            bounds,
            outer_stack,
        );
        self.release_scratch(mask);
        self.release_scratch(content_target);
        ok
    }

    fn render_ops_to_scratch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let target = self.acquire_scratch()?;
        self.clear_render_target(commands, target, 0);
        if self.execute_ops(commands, scene, plan, ops, target, filter_cursors) {
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
            filter.apply_region_mask(
                commands,
                self.render_target_view(mask),
                self.render_target_view(target),
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
        filter.composite_src_over_with_stack(
            commands,
            self.render_target_view(target),
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
        filter.composite_src_over_rect_mask_direct(
            commands,
            self.render_target_view(target),
            self.render_target_view(source),
            self.size,
            self.lengths,
            bounds,
            region,
        )
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
        filter.composite_blend_with_stack(
            commands,
            self.render_target_view(target),
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
        filter.composite_src_over_surface_with_stack(
            commands,
            self.render_target_view(target),
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

    fn render_target_view(&self, target: WgpuRenderTargetId) -> &::wgpu::TextureView {
        match target {
            WgpuRenderTargetId::Main => self
                .root_target_view
                .as_ref()
                .unwrap_or(self.readback_target.view()),
            WgpuRenderTargetId::Scratch(ix) => self.scratch[ix].view(),
        }
    }

    fn filter_brush_bindings(&self) -> WgpuFilterBrushBindings<'_> {
        WgpuFilterBrushBindings {
            data: self.filter_brushes.data.buffer(),
            params: self.filter_brushes.params.buffer(),
            payloads: self.filter_brushes.payloads.buffer(),
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

    pub fn render_with_text(&mut self, scene: &Canvas, text_context: &mut TextContext) {
        self.prepare_scene_with_text(scene, text_context);
        if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            return;
        }
        profile_cpu("cpu_fallback.render_text", || {
            self.cpu.render_with_text(scene, text_context)
        });
        self.upload_cpu_image();
    }

    pub fn render_with_options(
        &mut self,
        scene: &Canvas,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.prepare_scene(scene);
        let rendered_native = self.render_prepared_tile_plan(scene);
        if rendered_native {
            self.size = (scene.width, scene.height);
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

        let mut capture = self.cpu.render_with_options(scene, options);
        capture.backend = "wgpu".to_string();
        self.upload_cpu_image();
        capture
    }

    fn read_debug_scan_buffers(&self) -> WgpuDebugScanReadback {
        let backdrops =
            self.scan
                .backdrops
                .read::<i32>(&self.device, &self.queue, self.lengths.backdrop_len);
        let starts = self.scan.tile_segment_range_starts.read::<u32>(
            &self.device,
            &self.queue,
            self.lengths.backdrop_len,
        );
        let ends = self.scan.tile_segment_range_ends.read::<u32>(
            &self.device,
            &self.queue,
            self.lengths.backdrop_len,
        );
        let p0x = self.scan.segment_p0x.read::<f32>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        let p0y = self.scan.segment_p0y.read::<f32>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        let p1x = self.scan.segment_p1x.read::<f32>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        let p1y = self.scan.segment_p1y.read::<f32>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        let y_edge = self.scan.segment_y_edge.read::<f32>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        let tile_segment_ranges = starts
            .into_iter()
            .zip(ends)
            .map(|(start, end)| TileSegmentRange { start, end })
            .collect();
        let segments = p0x
            .into_iter()
            .zip(p0y)
            .zip(p1x)
            .zip(p1y)
            .zip(y_edge)
            .map(|((((p0x, p0y), p1x), p1y), y_edge)| LineSegment {
                point0: (p0x, p0y),
                point1: (p1x, p1y),
                y_edge,
            })
            .collect();
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

        let mapped = readback.slice(..).get_mapped_range();
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
        scene: &Canvas,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        if self.render_native_to_wgpu_texture(scene, dst) {
            return Ok(());
        }
        profile_cpu("cpu_fallback.render", || self.cpu.render(scene));
        self.size = (scene.width, scene.height);
        self.upload_image_to_wgpu_texture(dst, self.cpu.image())
    }

    pub fn render_with_text_to_wgpu_texture(
        &mut self,
        scene: &Canvas,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        if self.render_native_with_text_to_wgpu_texture(scene, text_context, dst) {
            return Ok(());
        }
        profile_cpu("cpu_fallback.render_text", || {
            self.cpu.render_with_text(scene, text_context)
        });
        self.size = (scene.width, scene.height);
        self.upload_image_to_wgpu_texture(dst, self.cpu.image())
    }

    fn render_native_to_wgpu_texture(&mut self, scene: &Canvas, dst: &::wgpu::Texture) -> bool {
        self.render_native_to_wgpu_texture_with_prepare(scene, dst, |renderer, scene| {
            renderer.prepare_scene(scene);
        })
    }

    fn render_native_with_text_to_wgpu_texture(
        &mut self,
        scene: &Canvas,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
    ) -> bool {
        self.render_native_to_wgpu_texture_with_prepare(scene, dst, |renderer, scene| {
            renderer.prepare_scene_with_text(scene, text_context);
        })
    }

    fn render_native_to_wgpu_texture_with_prepare(
        &mut self,
        scene: &Canvas,
        dst: &::wgpu::Texture,
        prepare: impl FnOnce(&mut Self, &Canvas),
    ) -> bool {
        if self
            .validate_wgpu_storage_texture_destination(dst, scene.width, scene.height)
            .is_err()
        {
            return false;
        }
        self.root_target_view = Some(dst.create_view(&::wgpu::TextureViewDescriptor::default()));
        prepare(self, scene);
        let rendered = if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            true
        } else {
            false
        };
        self.root_target_view = None;
        rendered
    }

    fn upload_cpu_image(&mut self) {
        let _profile_scope = start_cpu_scope("cpu_fallback.upload");
        let image = self.cpu.image();
        self.size = (image.width, image.height);
        self.readback_target
            .resize(&self.device, image.width, image.height);
        self.readback_target.upload(&self.queue, image);
    }

    fn upload_image_to_wgpu_texture(
        &self,
        dst: &::wgpu::Texture,
        image: &Image,
    ) -> Result<(), WgpuTextureRenderError> {
        let _profile_scope = start_cpu_scope("texture_upload");
        self.validate_wgpu_copy_texture_destination(dst, image.width, image.height)?;
        if image.pixels.is_empty() {
            return Ok(());
        }
        self.queue.write_texture(
            dst.as_image_copy(),
            bytemuck::cast_slice(&image.pixels),
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * std::mem::size_of::<u32>() as u32),
                rows_per_image: Some(image.height),
            },
            ::wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }

    fn validate_wgpu_copy_texture_destination(
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
        if !dst.usage().contains(::wgpu::TextureUsages::COPY_DST) {
            return Err(WgpuTextureRenderError::DestinationUsageMissing(dst.usage()));
        }
        if !matches!(
            dst.format(),
            ::wgpu::TextureFormat::Rgba8Unorm | ::wgpu::TextureFormat::Rgba8UnormSrgb
        ) || dst.dimension() != ::wgpu::TextureDimension::D2
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

fn draw_bounds(scene: &Canvas, draw_ix: usize) -> Bounds {
    let bounds = scene.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

#[cfg(test)]
mod tests;
