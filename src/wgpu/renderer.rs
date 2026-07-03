#![allow(clippy::too_many_arguments)]

use std::sync::mpsc;

use peniko::{Color, Mix};

use crate::{
    cpu::Renderer as CpuRenderer,
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    scene::Scene,
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
use super::cumsum::WgpuCumsumPipeline;
use super::filter::{
    FILTER_OPACITY, WgpuFilterBrushBindings, WgpuFilterPathBindings, WgpuFilterPipeline,
    WgpuFilterTurbulenceBindings, encode_color_filter, region_bounds,
};
use super::filter_resources::{
    WgpuFilterBrushBuffers, WgpuFilterConvolveBuffers, WgpuFilterCursors, WgpuFilterPathBuffers,
    WgpuFilterTransferBuffers, WgpuFilterTurbulenceBuffers,
};
use super::fine::{WgpuFinePipeline, premul_clear_color};
use super::scan::WgpuScanPipeline;
use super::scene::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuSceneUploadStaging};
use super::target::WgpuTarget;

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

    fn render(&mut self, scene: &Scene) {
        if self.render_native(scene) {
            return;
        }
        self.cpu.render(scene);
        self.upload_cpu_image();
    }

    fn execute(&mut self, scene: &Scene, _: Self::ExecuteArgs<'_>) {
        self.render(scene);
    }

    fn scan(&mut self, scene: &Scene, _: Self::ScanArgs<'_>) {
        if let Some(scan) = &self.scan_pipeline {
            scan.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        } else {
            <CpuRenderer as Render>::scan(&mut self.cpu, scene, ());
        }
    }

    fn cumsum(&mut self, scene: &Scene, _: Self::CumsumArgs<'_>) {
        if let Some(cumsum) = &self.cumsum {
            cumsum.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        } else {
            <CpuRenderer as Render>::cumsum(&mut self.cpu, scene, ());
        }
    }

    fn coarse(&mut self, _: &Scene, batch: Self::CoarseArgs<'_>) {
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
        let required_features =
            adapter.features() & ::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
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

    pub fn render(&mut self, scene: &Scene) {
        <Self as Render>::render(self, scene);
    }

    /// Renders only through native wgpu compute pipelines.
    ///
    /// This is useful for backend parity tests because `render` falls back to the
    /// CPU renderer when a scene still needs unsupported native coverage.
    pub fn render_native(&mut self, scene: &Scene) -> bool {
        self.prepare_scene(scene);
        self.render_prepared_native(scene)
    }

    /// Renders text scenes only through native wgpu compute pipelines.
    ///
    /// The caller supplies the text context so backend parity tests can compare
    /// native wgpu against another backend using exactly the same glyph atlas.
    pub fn render_native_with_text(
        &mut self,
        scene: &Scene,
        text_context: &mut TextContext,
    ) -> bool {
        self.prepare_scene_with_text(scene, text_context);
        self.render_prepared_native(scene)
    }

    fn render_prepared_native(&mut self, scene: &Scene) -> bool {
        if let Some(fine) = &self.fine
            && fine.render(
                &self.device,
                &self.queue,
                scene,
                &self.scene_buffers,
                &mut self.readback_target,
                self.clear_color,
            )
        {
            self.size = (scene.width, scene.height);
            return true;
        }
        if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            return true;
        }
        false
    }

    fn prepare_scene(&mut self, scene: &Scene) {
        self.text_data = None;
        self.prepare_scene_resources(scene);
    }

    fn prepare_scene_with_text(&mut self, scene: &Scene, text_context: &mut TextContext) {
        self.text_data = Some(PreparedTextData::new(
            &scene.text_glyphs,
            &scene.text_runs,
            text_context,
        ));
        self.prepare_scene_resources(scene);
    }

    fn prepare_scene_resources(&mut self, scene: &Scene) {
        self.size = (scene.width, scene.height);
        self.surface_origin = (0, 0);
        if self.root_target_view.is_none() {
            self.readback_target
                .resize(&self.device, scene.width, scene.height);
        }
        let lengths = GpuBufferLengths::from_scene_with_text(scene, self.text_data.as_ref());
        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        let (max_clip_depth, max_group_depth) = plan_stack_depths(&plan);
        self.scene_buffers.upload(
            &self.device,
            &self.queue,
            scene,
            &plan,
            self.text_data.as_ref(),
            &mut self.scene_upload,
        );
        self.scan.prepare_outputs(&self.device, lengths);
        self.coarse.prepare_outputs(&self.device, lengths);
        self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        self.prepare_scratch_buffers(required_scratch_count(&plan));
        self.filter_transfers
            .upload(&self.device, &self.queue, &plan);
        self.filter_brushes.upload(&self.device, &self.queue, &plan);
        self.filter_convolves
            .upload(&self.device, &self.queue, &plan);
        self.filter_turbulence
            .upload(&self.device, &self.queue, &plan);
        self.filter_paths.upload(&self.device, &self.queue, &plan);
        self.config.upload(
            &self.device,
            &self.queue,
            "tileink wgpu scene config",
            &[GpuSceneConfig::new(scene, lengths, self.clear_color)],
        );
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan);
    }

    fn activate_local_scene_resources(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        parent_filter: &Filter,
        scratch_count: usize,
        surface_origin: (i32, i32),
    ) -> SavedRendererState {
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

        let lengths = GpuBufferLengths::from_scene_with_text(scene, self.text_data.as_ref());
        let (max_clip_depth, max_group_depth) = plan_stack_depths(plan);
        self.size = (scene.width, scene.height);
        self.surface_origin = surface_origin;
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(plan.clone());
        self.scene_buffers.upload(
            &self.device,
            &self.queue,
            scene,
            plan,
            self.text_data.as_ref(),
            &mut self.scene_upload,
        );
        self.scan.prepare_outputs(&self.device, lengths);
        self.coarse.prepare_outputs(&self.device, lengths);
        self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        self.prepare_scratch_buffers(scratch_count.max(1));
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
        self.config.upload(
            &self.device,
            &self.queue,
            "tileink wgpu scene config",
            &[GpuSceneConfig::new(scene, lengths, self.clear_color)],
        );
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

    fn coarse_batch(
        &mut self,
        scene: &Scene,
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

    fn render_prepared_tile_plan(&mut self, scene: &Scene) -> bool {
        if self.fine.is_none() || self.coarse_pipeline.is_none() || self.filter.is_none() {
            return false;
        }
        let Some(plan) = self.plan.clone() else {
            return false;
        };

        <Self as Render>::scan(self, scene, ());
        <Self as Render>::cumsum(self, scene, ());
        self.clear_render_target(WgpuRenderTargetId::Main, self.clear_color);
        let mut filter_cursors = WgpuFilterCursors::default();
        self.execute_ops(
            scene,
            &plan,
            &plan.ops,
            WgpuRenderTargetId::Main,
            &mut filter_cursors,
        )
    }

    fn execute_ops(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        for op in ops {
            let ok = match op {
                ExecOp::DrawBatch { draws, layer_stack } => {
                    self.execute_draw_batch(scene, draws.clone(), layer_stack.clone(), target)
                }
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
        scene: &Scene,
        draws: std::ops::Range<usize>,
        layer_stack: std::ops::Range<usize>,
        target: WgpuRenderTargetId,
    ) -> bool {
        if draws.start >= draws.end {
            return true;
        }
        self.coarse_batch(
            scene,
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
        );
        self.fine_batch_to(target)
    }

    fn fine_batch_to(&mut self, target: WgpuRenderTargetId) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        match target {
            WgpuRenderTargetId::Main => {
                if let Some(target) = &self.root_target_view {
                    fine.render_tiles_to_view(
                        &self.device,
                        &self.queue,
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
                    fine.render_tiles(
                        &self.device,
                        &self.queue,
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
            WgpuRenderTargetId::Scratch(ix) => fine.render_tiles(
                &self.device,
                &self.queue,
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
        scene: &Scene,
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
        scene: &Scene,
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

        let Some(source) = self.render_ops_to_scratch(scene, plan, children, filter_cursors) else {
            return false;
        };
        if let Some(opacity) = opacity {
            self.apply_color_filter_to_target(source, bounds, FILTER_OPACITY, opacity);
        }

        let Some(mask) = self.acquire_scratch() else {
            self.release_scratch(source);
            return false;
        };
        self.clear_render_target(mask, 0);
        self.build_layer_mask(mask, draw as u32, bounds);
        let ok = if let Some(mode) = blend {
            self.composite_blend_with_stack(target, source, mask, bounds, outer_stack, mode)
        } else {
            self.composite_src_over_with_stack(target, source, Some(mask), bounds, outer_stack)
        };
        self.release_scratch(mask);
        self.release_scratch(source);
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_filter_layer(
        &mut self,
        scene: &Scene,
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
        let local = local_offscreen_scene(scene, plan, children, filter_bounds.surface);
        let local_filter = local_filter(filter, filter_bounds.surface);
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
        self.clear_render_target(source, 0);
        <Self as Render>::scan(self, &local.scene, ());
        <Self as Render>::cumsum(self, &local.scene, ());
        let mut local_filter_cursors = WgpuFilterCursors::default();
        let ok = self.execute_ops(
            &local.scene,
            &local.plan,
            &local.children,
            source,
            &mut local_filter_cursors,
        ) && self.apply_filter(
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
        scene: &Scene,
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

        let Some(backdrop) = self.acquire_scratch() else {
            return false;
        };
        self.clear_render_target(backdrop, 0);
        if !self.copy_region_to_target(target, backdrop, bounds) {
            self.release_scratch(backdrop);
            return false;
        }
        if !self.apply_filter(
            backdrop,
            bounds,
            filter,
            Some(sample_region),
            filter_cursors,
        ) {
            self.release_scratch(backdrop);
            return false;
        }

        let Some(mask) = self.acquire_scratch() else {
            self.release_scratch(backdrop);
            return false;
        };
        self.clear_render_target(mask, 0);
        let mask_ok = self.build_region_mask(mask, sample_region, path_index, bounds);
        if !mask_ok {
            self.release_scratch(mask);
            self.release_scratch(backdrop);
            return false;
        }

        let ok = self.composite_src_over_with_stack(
            target,
            backdrop,
            Some(mask),
            bounds,
            outer_stack.clone(),
        );
        self.release_scratch(mask);
        self.release_scratch(backdrop);
        ok && self.execute_ops(scene, plan, children, target, filter_cursors)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_mask_layer(
        &mut self,
        scene: &Scene,
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

        let Some(content_target) = self.render_ops_to_scratch(scene, plan, content, filter_cursors)
        else {
            return false;
        };
        let Some(mask_source) = self.render_ops_to_scratch(scene, plan, mask_ops, filter_cursors)
        else {
            self.release_scratch(content_target);
            return false;
        };

        let Some(mask) = self.acquire_scratch() else {
            self.release_scratch(mask_source);
            self.release_scratch(content_target);
            return false;
        };
        self.clear_render_target(mask, 0);
        self.svg_mask_coverage(mask_source, mask, bounds, layer.kind);
        self.release_scratch(mask_source);

        let Some(region_mask) = self.acquire_scratch() else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        };
        self.clear_render_target(region_mask, 0);
        let region_ok = self.build_region_mask(region_mask, &layer.region, path_index, bounds);
        if region_ok {
            self.apply_region_mask(region_mask, mask, bounds);
        }
        self.release_scratch(region_mask);
        if !region_ok {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        }

        let ok = self.composite_src_over_with_stack(
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
        scene: &Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let target = self.acquire_scratch()?;
        self.clear_render_target(target, 0);
        if self.execute_ops(scene, plan, ops, target, filter_cursors) {
            Some(target)
        } else {
            self.release_scratch(target);
            None
        }
    }

    fn apply_filter_graph(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        primitives: &[filter_model::FilterPrimitive],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        if primitives.is_empty() {
            return self.clear_render_region(target, bounds, 0);
        }

        let mut source_alpha = None;
        let mut outputs = Vec::with_capacity(primitives.len());
        for primitive in primitives {
            let Some(output) = self.apply_filter_graph_primitive(
                target,
                bounds,
                primitive,
                &outputs,
                &mut source_alpha,
                filter_cursors,
            ) else {
                for output in outputs {
                    self.release_scratch(output);
                }
                if let Some(source_alpha) = source_alpha {
                    self.release_scratch(source_alpha);
                }
                return false;
            };
            outputs.push(output);
        }

        let final_output = outputs[outputs.len() - 1];
        let ok = self.clear_render_region(target, bounds, 0)
            && self.copy_region_to_target(final_output, target, bounds);
        for output in outputs {
            self.release_scratch(output);
        }
        if let Some(source_alpha) = source_alpha {
            self.release_scratch(source_alpha);
        }
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_filter_graph_primitive(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        bounds: Bounds,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let region = primitive.region.intersect(bounds);
        match &primitive.kind {
            filter_model::FilterPrimitiveKind::Image { .. } => {
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.flood_region_to_target(output, region, filter_cursors.next_brush_index()) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Identity => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                self.copy_filter_graph_region(input, bounds, region)
            }
            filter_model::FilterPrimitiveKind::Filter(filter) => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let temp = self.acquire_scratch()?;
                self.clear_render_target(temp, 0);
                if !self.copy_region_to_target(input, temp, bounds)
                    || !self.apply_filter(temp, bounds, filter, None, filter_cursors)
                {
                    self.release_scratch(temp);
                    return None;
                }
                let output = self.copy_filter_graph_region(temp, bounds, region);
                self.release_scratch(temp);
                output
            }
            filter_model::FilterPrimitiveKind::Blend { mode } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.blend_filter_inputs(input, input2, output, region, *mode) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Composite { operator } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.composite_filter_inputs(input, input2, output, region, *operator) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Tile { source_region } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.tile_filter_input(input, output, region, *source_region) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Merge { inputs } => {
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                for input in inputs {
                    let input = self.resolve_filter_graph_input(
                        source_graphic,
                        *input,
                        outputs,
                        source_alpha,
                        bounds,
                    )?;
                    if !self.source_over_filter_input(input, output, region) {
                        self.release_scratch(output);
                        return None;
                    }
                }
                Some(output)
            }
            filter_model::FilterPrimitiveKind::DisplacementMap(displacement) => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.displacement_map_filter_inputs(input, input2, output, region, displacement)
                {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Turbulence(turbulence) => {
                let output = self.acquire_scratch()?;
                self.clear_render_target(output, 0);
                if self.turbulence_to_target(
                    output,
                    region,
                    turbulence,
                    filter_cursors.next_turbulence_index(),
                ) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
        }
    }

    fn resolve_required_filter_graph_input(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        bounds: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        self.resolve_filter_graph_input(
            source_graphic,
            primitive.input2?,
            outputs,
            source_alpha,
            bounds,
        )
    }

    fn resolve_filter_graph_input(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        input: filter_model::FilterInput,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        bounds: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        match input {
            filter_model::FilterInput::SourceGraphic => Some(source_graphic),
            filter_model::FilterInput::Primitive(index) => outputs.get(index).copied(),
            filter_model::FilterInput::SourceAlpha => {
                if let Some(target) = *source_alpha {
                    return Some(target);
                }
                let alpha = self.acquire_scratch()?;
                self.clear_render_target(alpha, 0);
                if self.source_alpha_to_target(source_graphic, alpha, bounds) {
                    *source_alpha = Some(alpha);
                    Some(alpha)
                } else {
                    self.release_scratch(alpha);
                    None
                }
            }
        }
    }

    fn copy_filter_graph_region(
        &mut self,
        input: WgpuRenderTargetId,
        bounds: Bounds,
        region: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        let output = self.acquire_scratch()?;
        self.clear_render_target(output, 0);
        if self.copy_region_to_target(input, output, region.intersect(bounds)) {
            Some(output)
        } else {
            self.release_scratch(output);
            None
        }
    }

    fn apply_filter(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&crate::shared::layer::region::Region>,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        match filter {
            Filter::Graph { primitives, .. } => {
                self.apply_filter_graph(target, bounds, primitives, filter_cursors)
            }
            Filter::Chain { filters, .. } => filters
                .iter()
                .all(|filter| self.apply_filter(target, bounds, filter, region, filter_cursors)),
            Filter::RectLiquidGlass(glass) => {
                let Some(glass_region) = rect_liquid_glass_region(region, bounds) else {
                    return false;
                };
                self.apply_liquid_glass(target, bounds, *glass, glass_region)
            }
            Filter::Offset { dx, dy } => {
                let dx = filter_model::filter_offset_to_pixel_delta(*dx);
                let dy = filter_model::filter_offset_to_pixel_delta(*dy);
                if dx == 0 && dy == 0 {
                    return true;
                }
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                self.clear_render_target(temp, 0);
                let ok = self.offset_region_to_target(target, temp, bounds, dx, dy)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
            } => {
                let std_dev_x = std_dev_x.max(0.0);
                let std_dev_y = std_dev_y.max(0.0);
                if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
                    return true;
                }
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                self.clear_render_target(temp, 0);
                let ok = match (std_dev_x > 0.0, std_dev_y > 0.0) {
                    (true, true) => {
                        self.blur_region_to_target(target, temp, bounds, std_dev_x, 0)
                            && self.blur_region_to_target(temp, target, bounds, std_dev_y, 1)
                    }
                    (true, false) => {
                        self.blur_region_to_target(target, temp, bounds, std_dev_x, 0)
                            && self.copy_region_to_target(temp, target, bounds)
                    }
                    (false, true) => {
                        self.blur_region_to_target(target, temp, bounds, std_dev_y, 1)
                            && self.copy_region_to_target(temp, target, bounds)
                    }
                    (false, false) => true,
                };
                self.release_scratch(temp);
                ok
            }
            Filter::ColorMatrix(matrix) => {
                self.apply_color_matrix_to_target(target, bounds, *matrix);
                true
            }
            Filter::ComponentTransfer(_) => {
                let table_index = filter_cursors.next_transfer_index();
                self.apply_component_transfer_to_target(target, bounds, table_index);
                true
            }
            Filter::ConvolveMatrix(matrix) => {
                let kernel_offset = filter_cursors.next_convolve_offset(matrix);
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                self.clear_render_target(temp, 0);
                let ok =
                    self.convolve_matrix_to_target(target, temp, bounds, matrix, kernel_offset)
                        && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::DiffuseLighting(lighting) => {
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                self.clear_render_target(temp, 0);
                let ok = self.diffuse_lighting_to_target(target, temp, bounds, lighting)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::SpecularLighting(lighting) => {
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                self.clear_render_target(temp, 0);
                let ok = self.specular_lighting_to_target(target, temp, bounds, lighting)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::Flood { .. } => {
                let brush_index = filter_cursors.next_brush_index();
                self.flood_region_to_target(target, bounds, brush_index)
            }
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                ..
            } => self.apply_drop_shadow(
                target,
                bounds,
                *offset_x,
                *offset_y,
                *std_dev,
                filter_cursors.next_brush_index(),
            ),
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => {
                let raw_radius_x = radius_x.max(0.0).ceil() as u32;
                let raw_radius_y = radius_y.max(0.0).ceil() as u32;
                if raw_radius_x == 0 && raw_radius_y == 0 {
                    return true;
                }
                if *operator == filter_model::MorphologyOperator::Erode
                    && (raw_radius_x.saturating_mul(2) >= self.size.0
                        || raw_radius_y.saturating_mul(2) >= self.size.1)
                {
                    return self.clear_render_target(target, 0);
                }

                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let Some(output) = self.acquire_scratch() else {
                    self.release_scratch(temp);
                    return false;
                };
                self.clear_render_target(temp, 0);
                self.clear_render_target(output, 0);
                let radius_x = raw_radius_x.min(self.size.0.saturating_sub(1));
                let radius_y = raw_radius_y.min(self.size.1.saturating_sub(1));
                let operator = encode_morphology_operator(*operator);
                let ok = self
                    .morphology_axis_to_target(target, temp, bounds, radius_x, operator, 0)
                    && self.morphology_axis_to_target(temp, output, bounds, radius_y, operator, 1)
                    && self.copy_region_to_target(output, target, bounds);
                self.release_scratch(output);
                self.release_scratch(temp);
                ok
            }
            _ => {
                let Some((filter_kind, amount)) = encode_color_filter(filter) else {
                    return false;
                };
                self.apply_color_filter_to_target(target, bounds, filter_kind, amount);
                true
            }
        }
    }

    fn clear_render_target(&self, target: WgpuRenderTargetId, color: u32) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.clear_buffer(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            color,
        );
        true
    }

    fn clear_render_region(&self, target: WgpuRenderTargetId, bounds: Bounds, color: u32) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.clear_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            color,
        );
        true
    }

    fn flood_region_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        brush_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        filter.flood_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            brush_index,
            &brushes,
        );
        true
    }

    fn apply_drop_shadow(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        offset_x: f32,
        offset_y: f32,
        std_dev: f32,
        brush_index: u32,
    ) -> bool {
        let Some(shadow) = self.acquire_scratch() else {
            return false;
        };
        self.clear_render_target(shadow, 0);
        if !self.build_drop_shadow_mask_to_target(
            target,
            shadow,
            bounds,
            offset_x.round() as i32,
            offset_y.round() as i32,
        ) {
            self.release_scratch(shadow);
            return false;
        }

        let std_dev = std_dev.max(0.0);
        if std_dev > 0.0 {
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(shadow);
                return false;
            };
            self.clear_render_target(temp, 0);
            let ok = self.blur_region_to_target(shadow, temp, bounds, std_dev, 0)
                && self.blur_region_to_target(temp, shadow, bounds, std_dev, 1);
            self.release_scratch(temp);
            if !ok {
                self.release_scratch(shadow);
                return false;
            }
        }

        let ok = self.composite_drop_shadow_to_target(target, shadow, bounds, brush_index);
        self.release_scratch(shadow);
        ok
    }

    fn build_drop_shadow_mask_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.build_drop_shadow_mask(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            dx,
            dy,
        );
        true
    }

    fn source_alpha_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.source_alpha_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    fn source_over_filter_input(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.source_over_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    fn blend_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        mode: Mix,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blend_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            mode,
        );
        true
    }

    fn composite_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        operator: filter_model::CompositeOperator,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.composite_inputs_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            operator,
        );
        true
    }

    fn displacement_map_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        displacement: &filter_model::DisplacementMap,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.displacement_map_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            displacement,
        );
        true
    }

    fn turbulence_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        turbulence: &filter_model::Turbulence,
        table_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let tables = self.filter_turbulence_bindings();
        filter.turbulence_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            turbulence,
            table_index,
            &tables,
        );
        true
    }

    fn tile_filter_input(
        &self,
        input: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        source_region: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.tile_region(
            &self.device,
            &self.queue,
            self.render_target_view(input),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            source_region,
        );
        true
    }

    fn composite_drop_shadow_to_target(
        &self,
        target: WgpuRenderTargetId,
        shadow: WgpuRenderTargetId,
        bounds: Bounds,
        brush_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        filter.composite_drop_shadow(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.render_target_view(shadow),
            self.size,
            self.lengths,
            bounds,
            brush_index,
            &brushes,
        );
        true
    }

    fn apply_color_matrix_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_color_matrix(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                matrix,
            );
        }
    }

    fn apply_component_transfer_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        table_index: u32,
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_component_transfer(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                table_index,
                self.filter_transfers.tables.buffer(),
            );
        }
    }

    fn convolve_matrix_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        matrix: &filter_model::ConvolveMatrix,
        kernel_offset: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.convolve_matrix_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            matrix,
            kernel_offset,
            self.filter_convolves.kernels.buffer(),
        );
        true
    }

    fn diffuse_lighting_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::DiffuseLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.diffuse_lighting_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            lighting,
            self.surface_origin,
        );
        true
    }

    fn specular_lighting_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::SpecularLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.specular_lighting_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            lighting,
            self.surface_origin,
        );
        true
    }

    fn apply_liquid_glass(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(source) = self.acquire_scratch() else {
            return false;
        };
        let Some(blurred) = self.acquire_scratch() else {
            self.release_scratch(source);
            return false;
        };
        self.clear_render_target(source, 0);
        self.clear_render_target(blurred, 0);
        let mut ok = self.copy_region_to_target(target, source, bounds)
            && self.copy_region_to_target(source, blurred, bounds);

        if ok && glass.blur_radius > 0 {
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(blurred);
                self.release_scratch(source);
                return false;
            };
            self.clear_render_target(temp, 0);
            let std_dev = glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
            ok = self.blur_region_to_target(blurred, temp, bounds, std_dev, 0)
                && self.blur_region_to_target(temp, blurred, bounds, std_dev, 1);
            self.release_scratch(temp);
        }

        ok = ok && self.liquid_glass_to_target(source, blurred, target, bounds, glass, region);
        self.release_scratch(blurred);
        self.release_scratch(source);
        ok
    }

    fn liquid_glass_to_target(
        &self,
        source: WgpuRenderTargetId,
        blurred: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.rect_liquid_glass_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(blurred),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            glass,
            region,
        );
        true
    }

    fn blur_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blur_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            std_dev,
            axis,
        );
        true
    }

    fn morphology_axis_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.morphology_axis_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            radius,
            operator,
            axis,
        );
        true
    }

    fn offset_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.offset_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            dx,
            dy,
        );
        true
    }

    fn copy_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.copy_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    fn apply_color_filter_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_color_filter(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                filter_kind,
                amount,
            );
        }
    }

    fn build_layer_mask(&self, target: WgpuRenderTargetId, draw_ix: u32, bounds: Bounds) {
        if let Some(filter) = &self.filter {
            let bindings = self.scene_buffers.filter_bindings(&self.scan);
            filter.build_layer_mask(
                &self.device,
                &self.queue,
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
        target: WgpuRenderTargetId,
        region: &crate::shared::layer::region::Region,
        path_index: Option<u32>,
        bounds: Bounds,
    ) -> bool {
        let paths = self.filter_path_bindings();
        self.filter.as_ref().is_some_and(|filter| {
            filter.build_region_mask(
                &self.device,
                &self.queue,
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
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        kind: crate::shared::layer::mask::MaskKind,
    ) {
        if let Some(filter) = &self.filter {
            filter.svg_mask_coverage(
                &self.device,
                &self.queue,
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
        mask: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_region_mask(
                &self.device,
                &self.queue,
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
            &self.device,
            &self.queue,
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

    fn composite_blend_with_stack(
        &self,
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
            &self.device,
            &self.queue,
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
            &self.device,
            &self.queue,
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

    pub fn render_with_text(&mut self, scene: &Scene, text_context: &mut TextContext) {
        self.prepare_scene_with_text(scene, text_context);
        if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            return;
        }
        self.cpu.render_with_text(scene, text_context);
        self.upload_cpu_image();
    }

    pub fn render_with_options(
        &mut self,
        scene: &Scene,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.prepare_scene(scene);
        let rendered_native = if let Some(fine) = &self.fine
            && fine.render(
                &self.device,
                &self.queue,
                scene,
                &self.scene_buffers,
                &mut self.readback_target,
                self.clear_color,
            ) {
            true
        } else {
            self.render_prepared_tile_plan(scene)
        };
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
        scene: &Scene,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        if self.render_native_to_wgpu_texture(scene, dst) {
            return Ok(());
        }
        self.cpu.render(scene);
        self.size = (scene.width, scene.height);
        self.upload_image_to_wgpu_texture(dst, self.cpu.image())
    }

    fn render_native_to_wgpu_texture(&mut self, scene: &Scene, dst: &::wgpu::Texture) -> bool {
        if self
            .validate_wgpu_storage_texture_destination(dst, scene.width, scene.height)
            .is_err()
        {
            return false;
        }
        self.root_target_view = Some(dst.create_view(&::wgpu::TextureViewDescriptor::default()));
        self.prepare_scene(scene);
        let rendered = if let Some(fine) = &self.fine
            && self.root_target_view.as_ref().is_some_and(|target| {
                fine.render_to_view(
                    &self.device,
                    &self.queue,
                    scene,
                    &self.scene_buffers,
                    target,
                    self.clear_color,
                )
            }) {
            self.size = (scene.width, scene.height);
            true
        } else if self.render_prepared_tile_plan(scene) {
            self.size = (scene.width, scene.height);
            true
        } else {
            false
        };
        self.root_target_view = None;
        rendered
    }

    fn upload_cpu_image(&mut self) {
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

fn draw_bounds(scene: &Scene, draw_ix: usize) -> Bounds {
    let bounds = scene.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color, Compose, Gradient, Mix,
        kurbo::{Affine, BezPath, Line, Rect, Shape},
    };

    use super::{Renderer, WgpuRenderTargetId};
    use crate::{
        FillRule, Scene, TextContext, TextLayoutOptions,
        cpu::Renderer as CpuRenderer,
        debug::{RenderDebugOptions, RenderOptions},
        render::Render,
        shared::{
            bounds::Bounds,
            brush::Brush,
            layer::{
                filter::{
                    COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ColorChannel,
                    CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
                    DisplacementMap, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind,
                    LightSource, MorphologyOperator, RectLiquidGlass, SpecularLighting, Turbulence,
                    TurbulenceKind,
                },
                mask::{Mask, MaskKind},
                region::Region,
            },
        },
    };

    #[test]
    fn wgpu_renderer_reads_uploaded_cpu_render_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_path(
            Rect::new(2.0, 2.0, 6.0, 6.0).to_path(0.0),
            Color::from_rgb8(220, 64, 72),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.push_path(
            Rect::new(6.0, 0.0, 8.0, 2.0).to_path(0.0),
            Color::from_rgb8(32, 96, 160),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);

        renderer.render(&scene);
        let image = renderer.image();

        assert!(renderer.scene_buffers.draw_flags_capacity() >= 8);
        assert_eq!(image.rgba8_at(3, 3), [220, 64, 72, 255]);
        assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_uploads_cpu_fallback_to_copy_texture_without_storage() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_path(
            Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
            Color::from_rgb8(10, 20, 30),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
        let texture = renderer
            .device()
            .create_texture(&::wgpu::TextureDescriptor {
                label: Some("tileink wgpu renderer test texture"),
                size: ::wgpu::Extent3d {
                    width: 8,
                    height: 8,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: ::wgpu::TextureDimension::D2,
                format: ::wgpu::TextureFormat::Rgba8Unorm,
                usage: ::wgpu::TextureUsages::COPY_DST | ::wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });

        renderer
            .render_to_wgpu_texture(&scene, &texture)
            .expect("render to wgpu texture");
        let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

        assert_eq!(&bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)], &[10, 20, 30, 255]);
    }

    #[test]
    fn wgpu_renderer_renders_tile_fine_directly_to_storage_texture_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_path(
            Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
            Color::from_rgb8(40, 100, 220),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
        if !renderer
            .device()
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            return;
        }
        let texture = renderer
            .device()
            .create_texture(&::wgpu::TextureDescriptor {
                label: Some("tileink wgpu renderer direct storage texture test"),
                size: ::wgpu::Extent3d {
                    width: 8,
                    height: 8,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: ::wgpu::TextureDimension::D2,
                format: ::wgpu::TextureFormat::Rgba8Unorm,
                usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });

        renderer
            .render_to_wgpu_texture(&scene, &texture)
            .expect("render directly to wgpu storage texture");
        let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

        assert_eq!(
            &bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)],
            &[40, 100, 220, 255]
        );
    }

    #[test]
    fn wgpu_renderer_renders_direct_fine_to_storage_texture_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_rect(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(12, 34, 56),
        );
        let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
        if !renderer
            .device()
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            return;
        }
        let texture = renderer
            .device()
            .create_texture(&::wgpu::TextureDescriptor {
                label: Some("tileink wgpu renderer direct fine storage texture test"),
                size: ::wgpu::Extent3d {
                    width: 8,
                    height: 8,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: ::wgpu::TextureDimension::D2,
                format: ::wgpu::TextureFormat::Rgba8Unorm,
                usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });

        renderer
            .render_to_wgpu_texture(&scene, &texture)
            .expect("render direct fine to wgpu storage texture");
        let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

        assert_eq!(&bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)], &[12, 34, 56, 255]);
    }

    #[test]
    fn wgpu_renderer_renders_offscreen_plan_directly_to_storage_texture() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 255, 0),
        );
        scene.push_filter_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        if !renderer
            .device()
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            return;
        }
        let texture = renderer
            .device()
            .create_texture(&::wgpu::TextureDescriptor {
                label: Some("tileink wgpu renderer offscreen storage texture test"),
                size: ::wgpu::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: ::wgpu::TextureDimension::D2,
                format: ::wgpu::TextureFormat::Rgba8Unorm,
                usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });

        renderer
            .render_to_wgpu_texture(&scene, &texture)
            .expect("render offscreen plan directly to wgpu storage texture");
        let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 16, 16);

        assert_eq!(
            &bytes[4 * (3 * 16 + 3)..4 * (3 * 16 + 4)],
            &[0, 255, 255, 255]
        );
        assert_eq!(
            &bytes[4 * (3 * 16 + 12)..4 * (3 * 16 + 13)],
            &[0, 255, 0, 255]
        );
    }

    #[test]
    fn wgpu_renderer_debug_capture_uses_native_scan_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(32, 32);
        scene.push_path(
            Rect::new(4.0, 4.0, 20.0, 20.0).to_path(0.1),
            Color::from_rgb8(0, 128, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );
        let options = RenderOptions {
            debug: Some(
                RenderDebugOptions::new("target/wgpu-debug-capture-test").with_tile((0, 0)),
            ),
        };
        let mut renderer = Renderer::new_default_device(32, 32, Color::TRANSPARENT);

        let capture = renderer.render_with_options(&scene, &options);

        assert_eq!(capture.backend, "wgpu");
        assert_eq!(capture.tiles.len(), 4);
        assert!(
            capture
                .tile
                .as_ref()
                .is_some_and(|tile| !tile.paths.is_empty())
        );
        assert!(capture.images.iter().any(|image| image.name == "final.png"));
    }

    #[test]
    fn wgpu_renderer_renders_sdf_primitives_in_fine_pass_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(2.0, 2.0, 14.0, 14.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 120, 220),
        );
        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        assert!(renderer.fine.is_some());

        renderer.render(&scene);
        let image = renderer.image();

        assert_eq!(image.rgba8_at(8, 8), [30, 120, 220, 255]);
        assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_samples_gradient_brush_in_fine_pass_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
            .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
        let mut scene = Scene::new(32, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            &gradient,
        );
        let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
        assert!(renderer.fine.is_some());

        renderer.render(&scene);
        let image = renderer.image();
        let left = image.rgba8_at(2, 8);
        let right = image.rgba8_at(29, 8);

        assert!(left[0] > left[2], "expected red side, got {left:?}");
        assert!(right[2] > right[0], "expected blue side, got {right:?}");
        assert_eq!(left[3], 255);
        assert_eq!(right[3], 255);
    }

    #[test]
    fn wgpu_scan_emits_segments_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_path(
            Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        if renderer.scan_pipeline.is_none() {
            return;
        }

        renderer.prepare_scene(&scene);
        <Renderer as Render>::scan(&mut renderer, &scene, ());

        let backdrops = renderer.scan.backdrops.read::<i32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.backdrop_len,
        );
        let starts = renderer.scan.tile_segment_range_starts.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.backdrop_len,
        );
        let ends = renderer.scan.tile_segment_range_ends.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.backdrop_len,
        );
        let segment_bumps = renderer.scan.segment_bumps.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.path_count,
        );
        let p0x = renderer.scan.segment_p0x.read::<f32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.segment_capacity,
        );
        let p1y = renderer.scan.segment_p1y.read::<f32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.segment_capacity,
        );

        assert_eq!(backdrops, vec![0]);
        assert_eq!(starts, vec![0]);
        assert_eq!(ends, vec![2]);
        assert_eq!(segment_bumps, vec![2]);
        assert!((p0x[0] - 4.0).abs() < 1e-3);
        assert!((p1y[0] - 16.0).abs() < 1e-6);
    }

    #[test]
    fn wgpu_cumsum_scans_backdrop_rows_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(48, 32);
        scene.push_path(
            Rect::new(0.0, 0.0, 48.0, 32.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let mut renderer = Renderer::new_default_device(48, 32, Color::TRANSPARENT);
        if renderer.cumsum.is_none() {
            return;
        }

        renderer.prepare_scene(&scene);
        let device = renderer.device().clone();
        let queue = renderer.queue().clone();
        renderer.scan.backdrops.upload(
            &device,
            &queue,
            "tileink wgpu cumsum test backdrops",
            &[1, -1, 2, 3, 0, -2],
        );
        <Renderer as Render>::cumsum(&mut renderer, &scene, ());

        assert_eq!(
            renderer.scan.backdrops.read::<i32>(
                renderer.device(),
                renderer.queue(),
                renderer.lengths.backdrop_len
            ),
            vec![1, 0, 2, 3, 3, 1]
        );
    }

    #[test]
    fn wgpu_coarse_emits_sdf_particles_for_rects_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(32, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 0, 255),
        );
        let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
        if renderer.coarse_pipeline.is_none() {
            return;
        }

        renderer.prepare_scene(&scene);
        renderer.coarse_batch(&scene, 0, scene.draw_records.len() as u32, 0, 0);

        assert_eq!(
            renderer.coarse.tile_ptcl_range_starts.read::<u32>(
                renderer.device(),
                renderer.queue(),
                renderer.lengths.tile_count
            ),
            vec![0, 2]
        );
        assert_eq!(
            renderer.coarse.tile_ptcl_range_ends.read::<u32>(
                renderer.device(),
                renderer.queue(),
                renderer.lengths.tile_count
            ),
            vec![2, 5]
        );
        assert_eq!(
            read_ptcl_tags(&renderer, 5),
            vec![
                crate::shared::gpu_types::CUBE_PTCL_SDF,
                crate::shared::gpu_types::CUBE_PTCL_END,
                crate::shared::gpu_types::CUBE_PTCL_SDF,
                crate::shared::gpu_types::CUBE_PTCL_SDF,
                crate::shared::gpu_types::CUBE_PTCL_END,
            ]
        );
        assert_eq!(
            renderer.coarse.ptcl_colors.read::<u32>(
                renderer.device(),
                renderer.queue(),
                renderer.lengths.coarse_ptcl_capacity
            ),
            vec![0, 0, 0, 1, 0]
        );
    }

    #[test]
    fn wgpu_renderer_applies_path_clip_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();
        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);

        renderer.render(&scene);
        let image = renderer.image();

        assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_opacity_layer_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_opacity_layer(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();
        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);

        renderer.render(&scene);
        let pixel = renderer.image().rgba8_at(8, 8);

        assert!(
            (126..=129).contains(&pixel[3]),
            "unexpected pixel {pixel:?}"
        );
        assert_eq!(pixel[1], 0);
        assert_eq!(pixel[2], 0);
    }

    #[test]
    fn wgpu_renderer_applies_blend_layer_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        let mut scene = Scene::new(16, 16);
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(200, 80, 40));
        scene.push_blend_layer(
            full.to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(64, 200, 180));
        scene.pop_layer();

        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);

        let mut cpu = CpuRenderer::new(16, 16, Color::TRANSPARENT);
        cpu.render(&scene);
        assert_images_near(&renderer.image(), &cpu.image(), 1, "multiply blend layer");
    }

    #[test]
    fn wgpu_renderer_applies_color_filter_to_offscreen_children_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_color_matrix_to_offscreen_children_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::ColorMatrix([
                0.0, 0.0, 0.0, 0.0, 0.0, //
                1.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ]),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_component_transfer_to_offscreen_children_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
        for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
            table[i] = 0;
            table[COMPONENT_TRANSFER_TABLE_SIZE + i] = if i == 0 { 255 } else { i as u32 };
            table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 0;
            table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = i as u32;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::ComponentTransfer(table),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_convolve_matrix_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(3, 1);
        scene.push_filter_layer(
            Filter::ConvolveMatrix(ConvolveMatrix {
                columns: 3,
                rows: 1,
                target_x: 1,
                target_y: 0,
                data: vec![1.0, 0.0, 0.0],
                divisor: 1.0,
                bias: 0.0,
                edge_mode: ConvolveEdgeMode::Duplicate,
                preserve_alpha: false,
            }),
            Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(10, 0, 0),
        );
        scene.push_rect(
            Rect::new(1.0, 0.0, 2.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(20, 0, 0),
        );
        scene.push_rect(
            Rect::new(2.0, 0.0, 3.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(40, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 0), [40, 0, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_diffuse_lighting_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(3, 1);
        scene.push_filter_layer(
            Filter::DiffuseLighting(DiffuseLighting {
                surface_scale: 1.0,
                diffuse_constant: 1.0,
                lighting_color: [1.0, 0.0, 0.0],
                light_source: LightSource::Distant {
                    azimuth: 180.0,
                    elevation: 0.0,
                },
            }),
            Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgba8(0, 0, 0, 0),
        );
        scene.push_rect(
            Rect::new(1.0, 0.0, 2.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgba8(0, 0, 0, 128),
        );
        scene.push_rect(
            Rect::new(2.0, 0.0, 3.0, 1.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let center = image.rgba8_at(1, 0);

        assert!(
            center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
            "expected red diffuse lighting at alpha slope center, got {center:?}"
        );
    }

    #[test]
    fn wgpu_renderer_applies_specular_lighting_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(1, 1);
        scene.push_filter_layer(
            Filter::SpecularLighting(SpecularLighting {
                surface_scale: 0.0,
                specular_constant: 0.5,
                specular_exponent: 1.0,
                lighting_color: [1.0, 0.5, 0.0],
                light_source: LightSource::Point {
                    x: 0.5,
                    y: 0.5,
                    z: 1.0,
                },
            }),
            Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(0, 0), [128, 64, 0, 128]);
    }

    #[test]
    fn wgpu_renderer_executes_filter_graph_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(8, 8),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::new(0, 0, 4, 8),
                        kind: FilterPrimitiveKind::Blend {
                            mode: Mix::Multiply,
                        },
                    },
                    FilterPrimitive {
                        input: FilterInput::Primitive(0),
                        input2: Some(FilterInput::SourceAlpha),
                        region: Bounds::new(4, 0, 8, 8),
                        kind: FilterPrimitiveKind::Composite {
                            operator: CompositeOperator::In,
                        },
                    },
                    FilterPrimitive {
                        input: FilterInput::Primitive(1),
                        input2: Some(FilterInput::Primitive(2)),
                        region: Bounds::canvas(8, 8),
                        kind: FilterPrimitiveKind::Composite {
                            operator: CompositeOperator::Over,
                        },
                    },
                ],
                fixed_region: true,
            },
            Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(2, 4), [0, 0, 0, 255]);
        assert_eq!(image.rgba8_at(6, 4), [0, 0, 255, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_filter_graph_displacement_map_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 2);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(0, 0, 8, 2),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgba8(255, 0, 0, 128)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::new(0, 0, 8, 2),
                        kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                            scale_x: 4.0,
                            scale_y: 0.0,
                            x_channel: ColorChannel::R,
                            y_channel: ColorChannel::A,
                            linear_rgb: false,
                        }),
                    },
                ],
                fixed_region: true,
            },
            Region::rect(Rect::new(0.0, 0.0, 8.0, 2.0), crate::Radius::ZERO),
        );
        for x in 0..8 {
            scene.push_rect(
                Rect::new(x as f64, 0.0, x as f64 + 1.0, 2.0),
                crate::Radius::ZERO,
                Color::from_rgb8((x as u8 + 1) * 20, 0, 0),
            );
        }
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let mut cpu = CpuRenderer::new(8, 2, Color::TRANSPARENT);
        cpu.render(&scene);

        assert_images_near(&image, &cpu.image(), 1, "filter graph displacement map");
    }

    #[test]
    fn wgpu_renderer_generates_filter_graph_turbulence_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(32, 24);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(6, 5, 28, 20),
                    kind: FilterPrimitiveKind::Turbulence(Turbulence {
                        stitch_tiles: true,
                        linear_rgb: true,
                        ..test_turbulence(TurbulenceKind::FractalNoise, -20, 4)
                    }),
                }],
                fixed_region: true,
            },
            Region::rect(Rect::new(4.0, 3.0, 30.0, 22.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(4.0, 3.0, 30.0, 22.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let mut cpu = CpuRenderer::new(32, 24, Color::TRANSPARENT);
        cpu.render(&scene);

        assert_images_near(&image, &cpu.image(), 1, "filter graph turbulence");
    }

    #[test]
    fn wgpu_renderer_filter_graph_turbulence_uses_surface_origin_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(80, 24);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(40, 4, 72, 20),
                    kind: FilterPrimitiveKind::Turbulence(Turbulence {
                        base_frequency_x: 0.09,
                        base_frequency_y: 0.13,
                        tile_x: 40.0,
                        tile_y: 4.0,
                        tile_width: 32.0,
                        tile_height: 16.0,
                        ..test_turbulence(TurbulenceKind::Turbulence, 5, 3)
                    }),
                }],
                fixed_region: true,
            },
            Region::rect(Rect::new(40.0, 4.0, 72.0, 20.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(40.0, 4.0, 72.0, 20.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let mut cpu = CpuRenderer::new(80, 24, Color::TRANSPARENT);
        cpu.render(&scene);

        assert_images_near(
            &image,
            &cpu.image(),
            1,
            "filter graph turbulence surface origin",
        );
    }

    #[test]
    fn wgpu_renderer_rasterizes_path_region_mask_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut triangle = BezPath::new();
        triangle.move_to((4.0, 4.0));
        triangle.line_to((12.0, 4.0));
        triangle.line_to((4.0, 12.0));
        triangle.close_path();
        let region = Region::path(triangle, Affine::IDENTITY, 0.0);
        let mut scene = Scene::new(16, 16);
        scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
        scene.pop_layer();

        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        assert_eq!(
            renderer
                .filter_paths
                .range_starts
                .read::<u32>(renderer.device(), renderer.queue(), 1),
            vec![0]
        );
        assert_eq!(
            renderer
                .filter_paths
                .range_ends
                .read::<u32>(renderer.device(), renderer.queue(), 1),
            vec![3]
        );
        assert_eq!(
            renderer
                .filter_paths
                .p0x
                .read::<i32>(renderer.device(), renderer.queue(), 3),
            vec![1024, 3072, 1024]
        );

        let mask = renderer.acquire_scratch().expect("scratch mask");
        renderer.clear_render_target(mask, 0);
        assert!(renderer.build_region_mask(mask, &region, Some(0), Bounds::new(4, 4, 12, 12)));

        let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
        assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
        assert_eq!(pixels[10 * 16 + 10], 0);
    }

    #[test]
    fn wgpu_renderer_rasterizes_nonzero_path_region_mask_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut path = BezPath::new();
        for _ in 0..2 {
            path.move_to((4.0, 4.0));
            path.line_to((12.0, 4.0));
            path.line_to((4.0, 12.0));
            path.close_path();
        }
        let region = Region::path(path, Affine::IDENTITY, 0.0);
        let mut scene = Scene::new(16, 16);
        scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
        scene.pop_layer();

        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        let mask = renderer.acquire_scratch().expect("scratch mask");
        renderer.clear_render_target(mask, 0);
        assert!(renderer.build_region_mask(mask, &region, Some(0), Bounds::new(4, 4, 12, 12)));

        let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
        assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
        assert_eq!(pixels[10 * 16 + 10], 0);
    }

    #[test]
    fn wgpu_renderer_rect_liquid_glass_backdrop_matches_cpu_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(64, 32);
        for x in 0..64 {
            let v = (x * 4) as u8;
            scene.push_rect(
                Rect::new(f64::from(x), 0.0, f64::from(x + 1), 32.0),
                crate::Radius::ZERO,
                Color::from_rgb8(v, v, v),
            );
        }
        scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass::default()),
            Region::rect(Rect::new(16.0, 4.0, 48.0, 28.0), crate::Radius::all(6.0)),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let mut cpu = CpuRenderer::new(64, 32, Color::TRANSPARENT);
        cpu.render(&scene);

        assert_images_near(&image, &cpu.image(), 4, "rect liquid glass backdrop");
    }

    #[test]
    fn wgpu_renderer_merges_filter_graph_inputs_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(8, 8),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(0, 0, 4, 8),
                        kind: FilterPrimitiveKind::Merge {
                            inputs: vec![FilterInput::Primitive(0), FilterInput::SourceGraphic],
                        },
                    },
                ],
                fixed_region: true,
            },
            Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(2, 4), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(6, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_tiles_filter_graph_input_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(6, 4);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 6, 4),
                    kind: FilterPrimitiveKind::Tile {
                        source_region: Bounds::new(1, 1, 3, 3),
                    },
                }],
                fixed_region: true,
            },
            Region::rect(Rect::new(0.0, 0.0, 6.0, 4.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(1.0, 1.0, 2.0, 2.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.push_rect(
            Rect::new(2.0, 1.0, 3.0, 2.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 255, 0),
        );
        scene.push_rect(
            Rect::new(1.0, 2.0, 2.0, 3.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 0, 255),
        );
        scene.push_rect(
            Rect::new(2.0, 2.0, 3.0, 3.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 255, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(0, 0), [255, 255, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(2, 0), [255, 255, 0, 255]);
        assert_eq!(image.rgba8_at(3, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_displaces_filter_graph_input_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(3, 1);
        scene.push_filter_layer(
            Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(0, 0, 3, 1),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::WHITE),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::new(0, 0, 3, 1),
                        kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                            scale_x: 2.0,
                            scale_y: 0.0,
                            x_channel: ColorChannel::R,
                            y_channel: ColorChannel::A,
                            linear_rgb: false,
                        }),
                    },
                ],
                fixed_region: true,
            },
            Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.push_rect(
            Rect::new(1.0, 0.0, 2.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 255, 0),
        );
        scene.push_rect(
            Rect::new(2.0, 0.0, 3.0, 1.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 0, 255),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_solid_flood_filter_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Flood {
                brush: Brush::Solid(Color::from_rgba8(20, 40, 80, 128)),
            },
            Region::rect(Rect::new(4.0, 4.0, 12.0, 12.0), crate::Radius::ZERO),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(8, 8), [10, 20, 40, 128]);
        assert_eq!(image.rgba8_at(2, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_samples_gradient_flood_filter_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let gradient = Gradient::new_linear((0.0, 0.0), (15.0, 0.0))
            .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Flood {
                brush: Brush::from_gradient(&gradient),
            },
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let left = image.rgba8_at(2, 8);
        let right = image.rgba8_at(13, 8);

        assert!(left[0] > left[2], "expected red side, got {left:?}");
        assert!(right[2] > right[0], "expected blue side, got {right:?}");
        assert_eq!(left[3], 255);
        assert_eq!(right[3], 255);
    }

    #[test]
    fn wgpu_renderer_applies_solid_drop_shadow_filter_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 2.0,
                offset_y: 1.0,
                std_dev: 0.0,
                brush: Brush::Solid(Color::from_rgba8(0, 0, 0, 128)),
            },
            Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(4.0, 4.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(9, 6), [0, 0, 0, 128]);
    }

    #[test]
    fn wgpu_renderer_samples_gradient_drop_shadow_filter_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
            .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
        let mut scene = Scene::new(32, 32);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 0.0,
                offset_y: 12.0,
                std_dev: 0.0,
                brush: Brush::from_gradient(&gradient),
            },
            Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 8.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let left = image.rgba8_at(4, 16);
        let right = image.rgba8_at(27, 16);

        assert!(left[0] > left[2], "expected red shadow side, got {left:?}");
        assert!(
            right[2] > right[0],
            "expected blue shadow side, got {right:?}"
        );
        assert_eq!(left[3], 255);
        assert_eq!(right[3], 255);
        assert_eq!(image.rgba8_at(4, 4), [255, 255, 255, 255]);
    }

    #[test]
    fn wgpu_renderer_isolates_opacity_layer_with_offscreen_child_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        let mut scene = Scene::new(16, 16);
        scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 128, 0));
        scene.push_filter_layer(
            Filter::Opacity(1.0),
            Region::rect(full, crate::Radius::ZERO),
        );
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
        scene.pop_layer();
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(8, 8), [0, 0, 128, 128]);
    }

    #[test]
    fn wgpu_renderer_isolates_blend_layer_with_offscreen_child_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        let mut scene = Scene::new(16, 16);
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
        scene.push_blend_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
        scene.push_filter_layer(
            Filter::Opacity(1.0),
            Region::rect(full, crate::Radius::ZERO),
        );
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 255, 0));
        scene.pop_layer();
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(4, 8), [0, 128, 0, 255]);
        assert_eq!(image.rgba8_at(12, 8), [128, 128, 128, 255]);
    }

    #[test]
    fn wgpu_renderer_isolates_plain_layer_with_child_blend_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        let mut scene = Scene::new(16, 16);
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
        scene.push_isolate_layer(full.to_path(0.0), Affine::IDENTITY, 0.0);
        scene.push_blend_layer(
            full.to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
        scene.pop_layer();
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_alpha_mask_layer_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut mask_scene = Scene::new(16, 16);
        mask_scene.push_rect(
            Rect::new(0.0, 0.0, 8.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgba8(255, 255, 255, 128),
        );

        let mut scene = Scene::new(16, 16);
        scene.push_mask_layer(
            mask_scene,
            Mask {
                region: Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
                kind: MaskKind::Alpha,
            },
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(4, 8), [128, 0, 0, 128]);
        assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_outer_clip_stack_to_offscreen_output_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.push_filter_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_outer_sdf_clip_stack_to_offscreen_output_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO);
        scene.push_filter_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_backdrop_filter_to_existing_target_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(48, 24);
        scene.push_rect(
            Rect::new(0.0, 0.0, 48.0, 24.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.push_backdrop_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(8.0, 4.0, 32.0, 20.0), crate::Radius::ZERO),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(12, 8), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_renders_backdrop_layer_children_after_filter_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(32, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.push_backdrop_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(4.0, 4.0, 12.0, 12.0),
            crate::Radius::ZERO,
            Color::from_rgb8(0, 255, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(2, 8), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
        assert_eq!(image.rgba8_at(24, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn wgpu_renderer_applies_offset_filter_to_offscreen_children_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Offset { dx: 2.0, dy: 1.0 },
            Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 4.0, 4.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(1, 2), [0, 0, 0, 0]);
        assert_eq!(image.rgba8_at(3, 2), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(6, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_blurs_offscreen_children_into_expanded_bounds_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let sample = Rect::new(24.0, 8.0, 40.0, 24.0);
        let mut scene = Scene::new(64, 32);
        scene.push_filter_layer(
            Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
            },
            Region::rect(sample, crate::Radius::ZERO),
        );
        scene.push_rect(sample, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
        scene.pop_layer();

        let image = render_native_wgpu(&scene);
        let expanded = image.rgba8_at(22, 16);

        assert!(
            expanded[0] > 0 && expanded[3] > 0,
            "expected blur outside source rect, got {expanded:?}"
        );
        assert_eq!(image.rgba8_at(12, 16), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_applies_morphology_filter_to_offscreen_children_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Morphology {
                radius_x: 1.0,
                radius_y: 1.0,
                operator: MorphologyOperator::Dilate,
            },
            Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
        );
        scene.push_rect(
            Rect::new(4.0, 4.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        scene.pop_layer();

        let image = render_native_wgpu(&scene);

        assert_eq!(image.rgba8_at(3, 5), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn wgpu_renderer_spills_deep_clip_stack_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut scene = Scene::new(32, 16);
        let depth = crate::shared::gpu_plan::FINE_LOCAL_CLIP_DEPTH + 2;
        for ix in 0..depth {
            scene.push_clip_layer(
                Rect::new(ix as f64 * 4.0, 0.0, 32.0, 16.0).to_path(0.0),
                Affine::IDENTITY,
                FillRule::NonZero,
                0.0,
            );
        }
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        for _ in 0..depth {
            scene.pop_layer();
        }

        let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
        renderer.render(&scene);

        let mut cpu = CpuRenderer::new(32, 16, Color::TRANSPARENT);
        cpu.render(&scene);
        assert_images_near(&renderer.image(), &cpu.image(), 1, "deep clip spill");
    }

    #[test]
    fn wgpu_renderer_spills_deep_opacity_stack_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        let mut scene = Scene::new(16, 16);
        let depth = crate::shared::gpu_plan::FINE_LOCAL_GROUP_DEPTH + 2;
        for _ in 0..depth {
            scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
        }
        scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
        for _ in 0..depth {
            scene.pop_layer();
        }

        let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);

        let mut cpu = CpuRenderer::new(16, 16, Color::TRANSPARENT);
        cpu.render(&scene);
        assert_images_near(&renderer.image(), &cpu.image(), 1, "deep opacity spill");
    }

    #[test]
    fn wgpu_renderer_draws_text_in_tile_fine_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut text_context = TextContext::new();
        let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
        if layout.is_empty() {
            return;
        }
        let mut scene = Scene::new(160, 64);
        scene.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
        let mut renderer = Renderer::new_default_device(160, 64, Color::TRANSPARENT);

        renderer.render_with_text(&scene, &mut text_context);
        let image = renderer.image();

        assert!(
            image.pixels.iter().any(|pixel| (pixel >> 24) != 0),
            "expected at least one text pixel"
        );
    }

    #[test]
    fn wgpu_renderer_matches_cpu_text_compositing_when_enabled() {
        if !run_wgpu_tests() {
            return;
        }

        let mut text_context = TextContext::new();
        let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
        if layout.is_empty() {
            return;
        }
        let mut scene = Scene::new(160, 64);
        scene.push_rect(
            Rect::new(0.0, 0.0, 160.0, 64.0),
            crate::Radius::ZERO,
            Color::from_rgb8(236, 238, 242),
        );
        scene.push_text_layout(
            &layout,
            peniko::kurbo::Point::new(8.0, 36.0),
            Color::from_rgb8(18, 24, 36),
        );

        let mut renderer = Renderer::new_default_device(160, 64, Color::TRANSPARENT);
        renderer.render_with_text(&scene, &mut text_context);
        let wgpu_image = renderer.image();

        let mut cpu = CpuRenderer::new(160, 64, Color::TRANSPARENT);
        cpu.render_with_text(&scene, &mut text_context);
        assert_images_near(&wgpu_image, &cpu.image(), 2, "linear text compositing");
    }

    fn run_wgpu_tests() -> bool {
        std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() == Ok("1")
            || std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() == Ok("1")
    }

    fn test_turbulence(kind: TurbulenceKind, seed: i32, num_octaves: u32) -> Turbulence {
        Turbulence {
            base_frequency_x: 0.07,
            base_frequency_y: 0.11,
            num_octaves,
            seed,
            stitch_tiles: false,
            kind,
            linear_rgb: false,
            transform_x: 0.0,
            transform_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            tile_x: 0.0,
            tile_y: 0.0,
            tile_width: 32.0,
            tile_height: 24.0,
        }
    }

    fn render_native_wgpu(scene: &Scene) -> crate::shared::image::Image {
        let mut renderer =
            Renderer::new_default_device(scene.width, scene.height, Color::TRANSPARENT);
        renderer.prepare_scene(scene);
        assert!(
            renderer.render_prepared_tile_plan(scene),
            "expected scene to render through native wgpu path"
        );
        renderer.image()
    }

    fn assert_images_near(
        actual: &crate::shared::image::Image,
        expected: &crate::shared::image::Image,
        tolerance: u8,
        context: &str,
    ) {
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        for y in 0..actual.height {
            for x in 0..actual.width {
                let a = actual.rgba8_at(x, y);
                let e = expected.rgba8_at(x, y);
                for channel in 0..4 {
                    let diff = a[channel].abs_diff(e[channel]);
                    assert!(
                        diff <= tolerance,
                        "{context} mismatch at ({x}, {y}) channel {channel}: actual {a:?}, expected {e:?}"
                    );
                }
            }
        }
    }

    fn read_render_target_u32(
        renderer: &Renderer,
        target: WgpuRenderTargetId,
        len: usize,
    ) -> Vec<u32> {
        let texture = match target {
            WgpuRenderTargetId::Main => renderer.readback_target.texture(),
            WgpuRenderTargetId::Scratch(ix) => renderer.scratch[ix].texture(),
        };
        let bytes = read_texture_rgba8(
            renderer.device(),
            renderer.queue(),
            texture,
            renderer.size.0,
            renderer.size.1,
        );
        let values = bytemuck::cast_slice(&bytes)[..len].to_vec();
        values
    }

    fn read_ptcl_tags(renderer: &Renderer, len: usize) -> Vec<u32> {
        let words = renderer.coarse.ptcl_tags.read::<u32>(
            renderer.device(),
            renderer.queue(),
            len.div_ceil(4),
        );
        (0..len)
            .map(|i| (words[i / 4] >> ((i % 4) * 8)) & 255)
            .collect()
    }

    fn read_texture_rgba8(
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        texture: &::wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let row_bytes = width as ::wgpu::BufferAddress * 4;
        let padded_row_bytes =
            row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
        let readback = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu renderer texture readback"),
            size: padded_row_bytes * height as u64,
            usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu renderer texture readback copy"),
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            ::wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: ::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: (height > 1).then_some(padded_row_bytes as u32),
                    rows_per_image: None,
                },
            },
            ::wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(::wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap()
            });
        device.poll(::wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();

        let view = readback.slice(..).get_mapped_range();
        let mut tight = Vec::with_capacity((row_bytes * height as u64) as usize);
        for row in 0..height as usize {
            let start = row * padded_row_bytes as usize;
            tight.extend_from_slice(&view[start..start + row_bytes as usize]);
        }
        drop(view);
        readback.unmap();
        tight
    }
}
