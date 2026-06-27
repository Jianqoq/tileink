use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, Color, kurbo::Shape};

use crate::{
    render::Render,
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        bounds::Bounds,
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry, ROOT_COMMAND_LIST_ID},
        fill::FillRule,
        image::{Image, premul_color_to_rgba8_pack},
        layer::{Layer, filter::Filter, region::Region},
        line::Line,
        path_flatten::PathFlatten,
        pixel::{opacity_f32_to_u8, premul_f32_to_u32},
        sdf::Sdf,
    },
};

use super::{
    brush::{GpuBrushBuffers, GpuBrushUpload},
    buffer::CubeBuffer,
    pipelines::{
        coarse::{CoarseBatch, CoarsePipeline},
        cumsum::CumsumPipeline,
        filter::{
            FILTER_BRIGHTNESS, FILTER_CONTRAST, FILTER_GRAYSCALE, FILTER_HUE_ROTATE, FILTER_INVERT,
            FILTER_OPACITY, FILTER_SATURATE, FILTER_SEPIA, FilterPathResources, FilterPipeline,
        },
        fine::{FinePipeline, FineRenderConfig},
        scan::ScanPipeline,
    },
    types::{
        CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_OPACITY, CUBE_LAYER_BLEND,
        CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY, CUBE_SDF_CIRCLE, CUBE_SDF_CIRCLE_STROKE,
        CUBE_SDF_NONE, CUBE_SDF_RECT, CUBE_SDF_RECT_STROKE, CubeBufferLengths, CubeSceneConfig,
        build_cumsum_plan, build_scan_chunks,
    },
};

pub type WgpuRenderer = Renderer<::cubecl::wgpu::WgpuRuntime>;

/// CubeCL renderer resource owner.
///
/// This replaces the direct wgpu backend. Kernels will be written with
/// `#[cube]` and launched through CubeCL; with the current Cargo features those
/// kernels compile to the CubeCL wgpu runtime.
pub struct Renderer<R: Runtime> {
    client: ::cubecl::client::ComputeClient<R>,
    clear: Color,
    clear_color: u32,
    size: (u32, u32),
    lengths: CubeBufferLengths,
    max_clip_depth: usize,
    max_group_depth: usize,
    plan: Option<ExecPlan>,
    config: CubeBuffer<CubeSceneConfig>,
    scene: SceneBuffers,
    scan: ScanBuffers,
    coarse: CoarseBuffers,
    draw_brushes: GpuBrushBuffers,
    filter_brushes: GpuBrushBuffers,
    filter_paths: FilterPathBuffers,
    target: CubeBuffer<u32>,
    scratch: Vec<CubeBuffer<u32>>,
    scratch_in_use: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CubeRenderTarget {
    Main,
    Scratch(usize),
}

#[cfg(feature = "bench-api")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubePreparedStage {
    Scan,
    Cumsum,
    Coarse,
    Fine,
}

impl<R: Runtime> Render for Renderer<R> {
    type ScanArgs<'a> = ();
    type CumsumArgs<'a> = ();
    type CoarseArgs<'a> = CoarseBatch;
    type FineArgs<'a> = ();
    type ExecuteArgs<'a> = ();

    fn render(&mut self, scene: &Scene) {
        self.prepare_scene(scene);
        <Self as Render>::scan(self, scene, ());
        <Self as Render>::cumsum(self, scene, ());
        self.clear_target();
        self.execute_prepared_plan(scene);
    }

    fn execute(&mut self, scene: &Scene, _: Self::ExecuteArgs<'_>) {
        <Self as Render>::render(self, scene);
    }

    fn scan(&mut self, _: &Scene, _: Self::ScanArgs<'_>) {
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    fn cumsum(&mut self, _: &Scene, _: Self::CumsumArgs<'_>) {
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    fn coarse(&mut self, _: &Scene, batch: Self::CoarseArgs<'_>) {
        CoarsePipeline::run(
            &self.client,
            &self.scene,
            &self.scan,
            &mut self.coarse,
            self.lengths,
            batch,
        );
    }

    fn fine(&mut self, _: &Scene, _: Self::FineArgs<'_>) {
        let config = self.fine_config();
        FinePipeline::run(
            &self.client,
            &self.scene,
            &self.scan,
            &self.coarse,
            self.draw_brushes.resources(),
            &mut self.target,
            config,
        );
    }
}

impl<R: Runtime> Renderer<R> {
    pub fn new(device: &R::Device, width: u32, height: u32, clear: Color) -> Self {
        let client = R::client(device);
        let clear_color = premul_color_to_rgba8_pack(clear);
        Self {
            config: CubeBuffer::new(&client, 1),
            scene: SceneBuffers::new(&client),
            scan: ScanBuffers::new(&client),
            coarse: CoarseBuffers::new(&client),
            draw_brushes: GpuBrushBuffers::new(&client),
            filter_brushes: GpuBrushBuffers::new(&client),
            filter_paths: FilterPathBuffers::new(&client),
            target: CubeBuffer::new(&client, width as usize * height as usize),
            scratch: Vec::new(),
            scratch_in_use: Vec::new(),
            client,
            clear,
            clear_color,
            size: (width, height),
            lengths: CubeBufferLengths::default(),
            max_clip_depth: 0,
            max_group_depth: 0,
            plan: None,
        }
    }

    /// Uploads immutable scene inputs and preallocates mutable stage buffers.
    ///
    /// This is the CubeCL version of the renderer memory contract: scene input
    /// buffers are uploaded once per scene, and every compute output has fixed
    /// capacity before any kernel is launched.
    pub fn prepare_scene(&mut self, scene: &Scene) {
        self.resize(scene.width, scene.height);
        let lengths = CubeBufferLengths::from_scene(scene);
        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        let (max_clip_depth, max_group_depth) = plan_stack_depths(&plan);
        let scratch_count = required_scratch_count(&plan);
        let draw_brush_upload = GpuBrushUpload::from_scene_draws(scene);
        let filter_brush_upload = GpuBrushUpload::from_filter_plan(&plan.ops);
        let filter_path_upload = FilterPathUpload::from_plan(&plan);
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.prepare_scratch_buffers(scratch_count);
        self.draw_brushes.upload(&self.client, draw_brush_upload);
        self.filter_brushes
            .upload(&self.client, filter_brush_upload);
        self.filter_paths.upload(&self.client, filter_path_upload);
        self.scene.upload(&self.client, scene, &plan);
        self.scan.prepare_outputs(&self.client, lengths);
        self.coarse.prepare_outputs(&self.client, lengths);
        self.config.replace(
            &self.client,
            &[CubeSceneConfig::new(scene, lengths, self.clear_color)],
        );
        self.plan = Some(plan);
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
            CoarseBatch {
                draw_start,
                draw_end,
                layer_stack_start,
                layer_stack_end,
            },
        );
    }

    fn clear_target(&mut self) {
        FinePipeline::clear(
            &self.client,
            &mut self.target,
            self.lengths,
            self.clear_color,
        );
    }

    fn fine_batch_to(&mut self, target: CubeRenderTarget) {
        let config = self.fine_config();
        match target {
            CubeRenderTarget::Main => FinePipeline::render(
                &self.client,
                &self.scene,
                &self.scan,
                &self.coarse,
                self.draw_brushes.resources(),
                &mut self.target,
                config,
            ),
            CubeRenderTarget::Scratch(ix) => FinePipeline::render(
                &self.client,
                &self.scene,
                &self.scan,
                &self.coarse,
                self.draw_brushes.resources(),
                &mut self.scratch[ix],
                config,
            ),
        }
    }

    fn fine_config(&self) -> FineRenderConfig {
        FineRenderConfig {
            lengths: self.lengths,
            size: self.size,
            clear_color: self.clear_color,
            max_clip_depth: self.max_clip_depth,
            max_group_depth: self.max_group_depth,
        }
    }

    pub fn render(&mut self, scene: &Scene) {
        <Self as Render>::render(self, scene);
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn run_prepared_stage_for_bench(&mut self, scene: &Scene, stage: CubePreparedStage) {
        let draw_end = self.lengths.draw_count as u32;
        match stage {
            CubePreparedStage::Scan => <Self as Render>::scan(self, scene, ()),
            CubePreparedStage::Cumsum => <Self as Render>::cumsum(self, scene, ()),
            CubePreparedStage::Coarse => <Self as Render>::coarse(
                self,
                scene,
                CoarseBatch {
                    draw_start: 0,
                    draw_end,
                    layer_stack_start: 0,
                    layer_stack_end: 0,
                },
            ),
            CubePreparedStage::Fine => <Self as Render>::fine(self, scene, ()),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        self.target
            .resize_uninit(&self.client, width as usize * height as usize);
        for scratch in &mut self.scratch {
            scratch.resize_uninit(&self.client, width as usize * height as usize);
        }
    }

    fn prepare_scratch_buffers(&mut self, scratch_count: usize) {
        let pixel_count = self.size.0 as usize * self.size.1 as usize;
        while self.scratch.len() < scratch_count {
            self.scratch
                .push(CubeBuffer::new(&self.client, pixel_count));
        }
        for scratch in &mut self.scratch {
            scratch.resize_uninit(&self.client, pixel_count);
        }
        self.scratch_in_use.clear();
        self.scratch_in_use.resize(self.scratch.len(), false);
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn clear_color(&self) -> Color {
        self.clear
    }

    pub fn buffer_lengths(&self) -> CubeBufferLengths {
        self.lengths
    }

    /// Reads the rendered target back into a CPU image.
    ///
    /// Rendering stays GPU-resident; this is the explicit readback point used by
    /// examples and consumers that need pixels on the CPU side.
    pub fn image(&self) -> Image {
        Image {
            width: self.size.0,
            height: self.size.1,
            pixels: self.target.read(&self.client),
        }
    }

    pub fn client(&self) -> &::cubecl::client::ComputeClient<R> {
        &self.client
    }

    pub fn runtime_name(&self) -> &'static str {
        R::name(&self.client)
    }

    fn execute_prepared_plan(&mut self, scene: &Scene) {
        let plan = self
            .plan
            .take()
            .expect("CubeCL execute requires prepare_scene to upload an execution plan first");
        self.execute_plan(scene, &plan);
        self.plan = Some(plan);
    }

    fn execute_plan(&mut self, scene: &Scene, plan: &ExecPlan) {
        let mut filter_brush_cursor = 0;
        let mut filter_path_cursor = 0;
        self.execute_ops(
            scene,
            plan,
            &plan.ops,
            CubeRenderTarget::Main,
            &mut filter_brush_cursor,
            &mut filter_path_cursor,
        );
    }

    fn execute_ops(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: CubeRenderTarget,
        filter_brush_cursor: &mut usize,
        filter_path_cursor: &mut usize,
    ) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => {
                    self.execute_draw_batch(scene, draws.clone(), layer_stack.clone(), target);
                }
                ExecOp::BeginClip
                | ExecOp::EndClip
                | ExecOp::BeginOpacity
                | ExecOp::EndOpacity
                | ExecOp::BeginBlend
                | ExecOp::EndBlend => {}
                ExecOp::OffscreenLayer {
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    scene,
                    plan,
                    layer,
                    outer_stack.clone(),
                    children,
                    target,
                    filter_brush_cursor,
                    filter_path_cursor,
                ),
            }
        }
    }

    fn execute_draw_batch(
        &mut self,
        scene: &Scene,
        draws: std::ops::Range<usize>,
        layer_stack: std::ops::Range<usize>,
        target: CubeRenderTarget,
    ) {
        if draws.start >= draws.end {
            return;
        }
        self.coarse_batch(
            scene,
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
        );
        self.fine_batch_to(target);
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_offscreen_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        layer: &Layer,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: CubeRenderTarget,
        filter_brush_cursor: &mut usize,
        filter_path_cursor: &mut usize,
    ) {
        match layer {
            Layer::Filter {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                let _path_index = next_filter_path_index(sample_region, filter_path_cursor);
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(
                    scene,
                    plan,
                    children,
                    source,
                    filter_brush_cursor,
                    filter_path_cursor,
                );
                let brush_index = next_filter_brush_index(filter, filter_brush_cursor);
                self.apply_filter(source, bounds, filter, brush_index);
                self.composite_src_over_with_stack(target, source, None, bounds, outer_stack);
                self.release_scratch(source);
            }
            Layer::Backdrop {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                let path_index = next_filter_path_index(sample_region, filter_path_cursor);
                let brush_index = next_filter_brush_index(filter, filter_brush_cursor);
                let backdrop = self.acquire_scratch();
                self.clear_buffer(backdrop, 0);
                self.copy_region(target, backdrop, bounds);
                self.apply_filter(backdrop, bounds, filter, brush_index);
                let mask = self.acquire_scratch();
                self.clear_buffer(mask, 0);
                self.build_region_mask(mask, sample_region, path_index, bounds);
                self.composite_src_over_with_stack(
                    target,
                    backdrop,
                    Some(mask),
                    bounds,
                    outer_stack.clone(),
                );
                self.release_scratch(mask);
                self.release_scratch(backdrop);

                let content = self.acquire_scratch();
                self.clear_buffer(content, 0);
                self.execute_ops(
                    scene,
                    plan,
                    children,
                    content,
                    filter_brush_cursor,
                    filter_path_cursor,
                );
                self.composite_src_over_with_stack(
                    target,
                    content,
                    None,
                    Bounds::canvas(self.size.0, self.size.1),
                    outer_stack,
                );
                self.release_scratch(content);
            }
            Layer::ClipSdf { .. } => {
                panic!("CubeCL ClipSdf layers are not implemented; use the CPU renderer")
            }
            _ => panic!("CubeCL offscreen execution only accepts Filter and Backdrop layers"),
        }
    }

    fn acquire_scratch(&mut self) -> CubeRenderTarget {
        for (ix, in_use) in self.scratch_in_use.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return CubeRenderTarget::Scratch(ix);
            }
        }
        panic!("CubeCL renderer ran out of preallocated offscreen scratch buffers");
    }

    fn release_scratch(&mut self, target: CubeRenderTarget) {
        let CubeRenderTarget::Scratch(ix) = target else {
            return;
        };
        self.scratch_in_use[ix] = false;
    }

    fn apply_filter(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        filter: &Filter,
        brush_index: Option<u32>,
    ) {
        match filter {
            Filter::Blur(radius) => {
                if radius.max(0.0) > 0.0 {
                    let temp = self.acquire_scratch();
                    self.blur_buffer(target, temp, bounds, *radius);
                    self.release_scratch(temp);
                }
            }
            Filter::DropShadow {
                offset_x,
                offset_y,
                radius,
                ..
            } => self.apply_drop_shadow(
                target,
                bounds,
                *offset_x,
                *offset_y,
                *radius,
                brush_index.expect("prepared DropShadow filter brush index is missing"),
            ),
            _ => {
                let (filter_kind, amount) = encode_color_filter(filter);
                self.apply_color_filter(target, bounds, filter_kind, amount);
            }
        }
    }

    fn clear_buffer(&mut self, target: CubeRenderTarget, clear_color: u32) {
        match target {
            CubeRenderTarget::Main => {
                FinePipeline::clear(&self.client, &mut self.target, self.lengths, clear_color)
            }
            CubeRenderTarget::Scratch(ix) => FinePipeline::clear(
                &self.client,
                &mut self.scratch[ix],
                self.lengths,
                clear_color,
            ),
        }
    }

    fn apply_color_filter(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        match target {
            CubeRenderTarget::Main => FilterPipeline::apply_color_filter(
                &self.client,
                &mut self.target,
                self.size,
                bounds,
                filter_kind,
                amount,
            ),
            CubeRenderTarget::Scratch(ix) => FilterPipeline::apply_color_filter(
                &self.client,
                &mut self.scratch[ix],
                self.size,
                bounds,
                filter_kind,
                amount,
            ),
        }
    }

    fn copy_region(&mut self, source: CubeRenderTarget, target: CubeRenderTarget, bounds: Bounds) {
        if source == target {
            return;
        }
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::copy_region(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::copy_region(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::copy_region(&self.client, source, target, self.size, bounds)
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn blur_buffer(
        &mut self,
        target: CubeRenderTarget,
        temp: CubeRenderTarget,
        bounds: Bounds,
        radius: f32,
    ) {
        if radius.max(0.0) <= 0.0 {
            return;
        }
        self.blur_pass(target, temp, bounds, radius, 0);
        self.blur_pass(temp, target, bounds, radius, 1);
    }

    fn blur_pass(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        radius: f32,
        axis: u32,
    ) {
        if source == target {
            return;
        }
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::blur_pass(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                    radius,
                    axis,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::blur_pass(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                    radius,
                    axis,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::blur_pass(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    radius,
                    axis,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn apply_drop_shadow(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        offset_x: f32,
        offset_y: f32,
        radius: f32,
        brush_index: u32,
    ) {
        let shadow = self.acquire_scratch();
        self.clear_buffer(shadow, 0);
        self.build_drop_shadow_mask(
            target,
            shadow,
            bounds,
            offset_x.round() as i32,
            offset_y.round() as i32,
        );

        if radius.max(0.0) > 0.0 {
            let temp = self.acquire_scratch();
            self.blur_buffer(shadow, temp, bounds, radius);
            self.release_scratch(temp);
        }

        self.composite_drop_shadow(target, shadow, bounds, brush_index);
        self.release_scratch(shadow);
    }

    fn build_drop_shadow_mask(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        if source == target {
            return;
        }
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::build_drop_shadow_mask(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                    dx,
                    dy,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::build_drop_shadow_mask(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                    dx,
                    dy,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::build_drop_shadow_mask(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    dx,
                    dy,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn composite_drop_shadow(
        &mut self,
        target: CubeRenderTarget,
        shadow: CubeRenderTarget,
        bounds: Bounds,
        brush_index: u32,
    ) {
        if target == shadow {
            return;
        }
        match (target, shadow) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(shadow_ix)) => {
                let brushes = self.filter_brushes.resources();
                FilterPipeline::composite_drop_shadow(
                    &self.client,
                    &mut self.target,
                    &self.scratch[shadow_ix],
                    self.size,
                    bounds,
                    brush_index,
                    brushes,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Main) => {
                let brushes = self.filter_brushes.resources();
                FilterPipeline::composite_drop_shadow(
                    &self.client,
                    &mut self.scratch[target_ix],
                    &self.target,
                    self.size,
                    bounds,
                    brush_index,
                    brushes,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Scratch(shadow_ix)) => {
                let (shadow, target) =
                    scratch_source_target(&mut self.scratch, shadow_ix, target_ix);
                let brushes = self.filter_brushes.resources();
                FilterPipeline::composite_drop_shadow(
                    &self.client,
                    target,
                    shadow,
                    self.size,
                    bounds,
                    brush_index,
                    brushes,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn composite_src_over(
        &mut self,
        target: CubeRenderTarget,
        source: CubeRenderTarget,
        bounds: Bounds,
    ) {
        if source == target {
            return;
        }
        match (target, source) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(source_ix)) => {
                FilterPipeline::composite_src_over_region(
                    &self.client,
                    &mut self.target,
                    &self.scratch[source_ix],
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Main) => {
                FilterPipeline::composite_src_over_region(
                    &self.client,
                    &mut self.scratch[target_ix],
                    &self.target,
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Scratch(source_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::composite_src_over_region(
                    &self.client,
                    target,
                    source,
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn composite_src_over_with_stack(
        &mut self,
        target: CubeRenderTarget,
        source: CubeRenderTarget,
        mask: Option<CubeRenderTarget>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) {
        if layer_stack.is_empty() && mask.is_none() {
            self.composite_src_over(target, source, bounds);
            return;
        }
        if source == target {
            return;
        }
        match (target, source, mask) {
            (
                CubeRenderTarget::Main,
                CubeRenderTarget::Scratch(source_ix),
                Some(CubeRenderTarget::Scratch(mask_ix)),
            ) => FilterPipeline::composite_src_over_stack_region(
                &self.client,
                &self.scene,
                &self.scan,
                &mut self.target,
                &self.scratch[source_ix],
                Some(&self.scratch[mask_ix]),
                self.size,
                bounds,
                layer_stack.start as u32,
                layer_stack.end as u32,
                self.max_group_depth,
            ),
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(source_ix), None) => {
                FilterPipeline::composite_src_over_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    &mut self.target,
                    &self.scratch[source_ix],
                    None,
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
            }
            (
                CubeRenderTarget::Scratch(target_ix),
                CubeRenderTarget::Main,
                Some(CubeRenderTarget::Scratch(mask_ix)),
            ) => {
                let (target, mask) =
                    scratch_target_and_source(&mut self.scratch, target_ix, mask_ix);
                FilterPipeline::composite_src_over_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    target,
                    &self.target,
                    Some(mask),
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Main, None) => {
                FilterPipeline::composite_src_over_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    &mut self.scratch[target_ix],
                    &self.target,
                    None,
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
            }
            (
                CubeRenderTarget::Scratch(target_ix),
                CubeRenderTarget::Scratch(source_ix),
                Some(CubeRenderTarget::Scratch(mask_ix)),
            ) => {
                let (target, source, mask) =
                    scratch_target_source_mask(&mut self.scratch, target_ix, source_ix, mask_ix);
                FilterPipeline::composite_src_over_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    target,
                    source,
                    Some(mask),
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
            }
            (CubeRenderTarget::Scratch(target_ix), CubeRenderTarget::Scratch(source_ix), None) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::composite_src_over_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    target,
                    source,
                    None,
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main, _)
            | (_, _, Some(CubeRenderTarget::Main)) => {
                panic!("CubeCL stack composite requires scratch source and scratch mask")
            }
        }
    }

    fn build_region_mask(
        &mut self,
        target: CubeRenderTarget,
        region: &Region,
        path_index: Option<u32>,
        bounds: Bounds,
    ) {
        let CubeRenderTarget::Scratch(target_ix) = target else {
            panic!("CubeCL region masks must be rendered into preallocated scratch");
        };
        match region {
            Region::Rect { rect, radius } => FilterPipeline::rasterize_rect_mask(
                &self.client,
                &mut self.scratch[target_ix],
                self.size,
                bounds,
                (
                    rect.x0 as f32,
                    rect.y0 as f32,
                    rect.x1 as f32,
                    rect.y1 as f32,
                ),
                (
                    radius.top_left,
                    radius.top_right,
                    radius.bottom_left,
                    radius.bottom_right,
                ),
            ),
            Region::Path { .. } => FilterPipeline::rasterize_path_mask(
                &self.client,
                &mut self.scratch[target_ix],
                self.size,
                bounds,
                path_index.expect("prepared Region::Path mask index is missing"),
                self.filter_paths.resources(),
            ),
        }
    }
}

fn plan_stack_depths(plan: &ExecPlan) -> (usize, usize) {
    plan_stack_depths_for_ops(&plan.ops, plan)
}

fn plan_stack_depths_for_ops(ops: &[ExecOp], plan: &ExecPlan) -> (usize, usize) {
    let mut max_clip_depth = 0;
    let mut max_group_depth = 0;
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[layer_stack.clone()]);
                max_clip_depth = max_clip_depth.max(clip_depth);
                max_group_depth = max_group_depth.max(group_depth);
            }
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[outer_stack.clone()]);
                let (child_clip_depth, child_group_depth) =
                    plan_stack_depths_for_ops(children, plan);
                max_clip_depth = max_clip_depth.max(clip_depth).max(child_clip_depth);
                max_group_depth = max_group_depth.max(group_depth).max(child_group_depth);
            }
            _ => {}
        }
    }
    (max_clip_depth, max_group_depth)
}

fn layer_stack_depths(entries: &[LayerStackEntry]) -> (usize, usize) {
    (
        entries
            .iter()
            .filter(|entry| matches!(entry, LayerStackEntry::Clip { .. }))
            .count(),
        entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. }
                )
            })
            .count(),
    )
}

fn required_scratch_count(plan: &ExecPlan) -> usize {
    max_scratch_for_ops(&plan.ops, 0)
}

fn max_scratch_for_ops(ops: &[ExecOp], held: usize) -> usize {
    let mut max_count = held;
    for op in ops {
        if let ExecOp::OffscreenLayer {
            layer,
            outer_stack,
            children,
        } = op
        {
            match layer {
                Layer::Filter { filter, .. } => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + filter_scratch_extra(filter));
                    if !outer_stack.is_empty() {
                        max_count = max_count.max(source_held);
                    }
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                Layer::Backdrop { filter, .. } => {
                    let backdrop_held = held + 1;
                    max_count = max_count.max(backdrop_held + filter_scratch_extra(filter));
                    max_count = max_count.max(backdrop_held + 1);
                    let content_held = held + 1;
                    max_count = max_count.max(max_scratch_for_ops(children, content_held));
                }
                _ => {
                    max_count = max_count.max(max_scratch_for_ops(children, held));
                }
            }
        }
    }
    max_count
}

fn filter_scratch_extra(filter: &Filter) -> usize {
    match filter {
        Filter::Blur(radius) => usize::from(radius.max(0.0) > 0.0),
        Filter::DropShadow { radius, .. } => 1 + usize::from(radius.max(0.0) > 0.0),
        _ => 0,
    }
}

fn encode_layer_payload(entry: LayerStackEntry) -> u32 {
    match entry {
        LayerStackEntry::Clip { .. } => 0,
        LayerStackEntry::Opacity { opacity, .. } => opacity_f32_to_u8(opacity) as u32,
        LayerStackEntry::Blend { mode, .. } => encode_blend_mode(mode),
    }
}

fn encode_blend_mode(mode: BlendMode) -> u32 {
    mode.mix as u32 | ((mode.compose as u32) << 8)
}

fn encode_color_filter(filter: &Filter) -> (u32, f32) {
    match filter {
        Filter::Brightness(amount) => (FILTER_BRIGHTNESS, *amount),
        Filter::Contrast(amount) => (FILTER_CONTRAST, *amount),
        Filter::Grayscale(amount) => (FILTER_GRAYSCALE, *amount),
        Filter::HueRotate(amount) => (FILTER_HUE_ROTATE, *amount),
        Filter::Invert(amount) => (FILTER_INVERT, *amount),
        Filter::Opacity(amount) => (FILTER_OPACITY, *amount),
        Filter::Saturate(amount) => (FILTER_SATURATE, *amount),
        Filter::Sepia(amount) => (FILTER_SEPIA, *amount),
        Filter::Blur(_) => panic!("blur is handled by CubeCL separable blur passes"),
        Filter::DropShadow { .. } => {
            panic!("drop-shadow is handled by the CubeCL shadow-mask passes")
        }
    }
}

fn supported_filter_bounds(filter: &Filter, sample_region: &Region, size: (u32, u32)) -> Bounds {
    let bounds = region_bounds(sample_region);
    let outset = match filter {
        Filter::Blur(radius) => blur_outset(*radius),
        Filter::DropShadow {
            radius,
            offset_x,
            offset_y,
            ..
        } => blur_outset(*radius) + offset_x.abs().ceil().max(offset_y.abs().ceil()) as i32,
        _ => 0,
    };
    bounds
        .outset(outset)
        .intersect(Bounds::canvas(size.0, size.1))
}

fn next_filter_brush_index(filter: &Filter, cursor: &mut usize) -> Option<u32> {
    if matches!(filter, Filter::DropShadow { .. }) {
        let index = *cursor as u32;
        *cursor += 1;
        Some(index)
    } else {
        None
    }
}

fn next_filter_path_index(region: &Region, cursor: &mut usize) -> Option<u32> {
    if matches!(region, Region::Path { .. }) {
        let index = *cursor as u32;
        *cursor += 1;
        Some(index)
    } else {
        None
    }
}

struct FilterPathBuffers {
    range_starts: CubeBuffer<u32>,
    range_ends: CubeBuffer<u32>,
    p0x: CubeBuffer<i32>,
    p0y: CubeBuffer<i32>,
    p1x: CubeBuffer<i32>,
    p1y: CubeBuffer<i32>,
}

impl FilterPathBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            range_starts: CubeBuffer::new(client, 0),
            range_ends: CubeBuffer::new(client, 0),
            p0x: CubeBuffer::new(client, 0),
            p0y: CubeBuffer::new(client, 0),
            p1x: CubeBuffer::new(client, 0),
            p1y: CubeBuffer::new(client, 0),
        }
    }

    fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        upload: FilterPathUpload,
    ) {
        self.range_starts.replace(client, &upload.range_starts);
        self.range_ends.replace(client, &upload.range_ends);
        self.p0x.replace(client, &upload.p0x);
        self.p0y.replace(client, &upload.p0y);
        self.p1x.replace(client, &upload.p1x);
        self.p1y.replace(client, &upload.p1y);
    }

    fn resources(&self) -> FilterPathResources<'_> {
        FilterPathResources {
            range_starts: &self.range_starts,
            range_ends: &self.range_ends,
            p0x: &self.p0x,
            p0y: &self.p0y,
            p1x: &self.p1x,
            p1y: &self.p1y,
        }
    }
}

#[derive(Default)]
struct FilterPathUpload {
    range_starts: Vec<u32>,
    range_ends: Vec<u32>,
    p0x: Vec<i32>,
    p0y: Vec<i32>,
    p1x: Vec<i32>,
    p1y: Vec<i32>,
}

impl FilterPathUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_paths_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_region(&mut self, region: &Region) {
        let Region::Path {
            path,
            transform,
            tolerance,
        } = region
        else {
            return;
        };

        let start = self.p0x.len() as u32;
        let path = *transform * path;
        let mut tile_count = 0;
        let mut lines = Vec::new();
        PathFlatten::new(
            &path,
            *tolerance as f32,
            self.range_starts.len() as u32,
            &mut tile_count,
        )
        .flatten(&mut lines);
        self.p0x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[0])),
        );
        self.p0y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p0[1])),
        );
        self.p1x.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[0])),
        );
        self.p1y.extend(
            lines
                .iter()
                .map(|line| encode_filter_path_coord(line.p1[1])),
        );
        self.range_starts.push(start);
        self.range_ends.push(self.p0x.len() as u32);
    }
}

fn encode_filter_path_coord(value: f32) -> i32 {
    (value * 256.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn collect_filter_paths_for_ops(ops: &[ExecOp], upload: &mut FilterPathUpload) {
    for op in ops {
        if let ExecOp::OffscreenLayer {
            layer, children, ..
        } = op
        {
            match layer {
                Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. } => {
                    upload.push_region(sample_region);
                    collect_filter_paths_for_ops(children, upload);
                }
                _ => collect_filter_paths_for_ops(children, upload),
            }
        }
    }
}

fn region_bounds(region: &Region) -> Bounds {
    match region {
        Region::Rect { rect, .. } => Bounds::new(
            rect.x0.floor() as i32,
            rect.y0.floor() as i32,
            rect.x1.ceil() as i32,
            rect.y1.ceil() as i32,
        ),
        Region::Path {
            path, transform, ..
        } => {
            let rect = transform.transform_rect_bbox(path.bounding_box());
            Bounds::new(
                rect.x0.floor() as i32,
                rect.y0.floor() as i32,
                rect.x1.ceil() as i32,
                rect.y1.ceil() as i32,
            )
        }
    }
}

fn blur_outset(radius: f32) -> i32 {
    (radius.max(0.0) * 3.0).ceil() as i32
}

fn scratch_source_target(
    scratch: &mut [CubeBuffer<u32>],
    source_ix: usize,
    target_ix: usize,
) -> (&CubeBuffer<u32>, &mut CubeBuffer<u32>) {
    assert_ne!(
        source_ix, target_ix,
        "source and target scratch buffers must differ"
    );
    if source_ix < target_ix {
        let (left, right) = scratch.split_at_mut(target_ix);
        (&left[source_ix], &mut right[0])
    } else {
        let (left, right) = scratch.split_at_mut(source_ix);
        (&right[0], &mut left[target_ix])
    }
}

fn scratch_target_and_source(
    scratch: &mut [CubeBuffer<u32>],
    target_ix: usize,
    source_ix: usize,
) -> (&mut CubeBuffer<u32>, &CubeBuffer<u32>) {
    assert_ne!(
        target_ix, source_ix,
        "target and source scratch buffers must differ"
    );
    if target_ix < source_ix {
        let (left, right) = scratch.split_at_mut(source_ix);
        (&mut left[target_ix], &right[0])
    } else {
        let (left, right) = scratch.split_at_mut(target_ix);
        (&mut right[0], &left[source_ix])
    }
}

fn scratch_target_source_mask(
    scratch: &mut [CubeBuffer<u32>],
    target_ix: usize,
    source_ix: usize,
    mask_ix: usize,
) -> (&mut CubeBuffer<u32>, &CubeBuffer<u32>, &CubeBuffer<u32>) {
    assert_ne!(
        target_ix, source_ix,
        "target and source scratch buffers must differ"
    );
    assert_ne!(
        target_ix, mask_ix,
        "target and mask scratch buffers must differ"
    );
    assert_ne!(
        source_ix, mask_ix,
        "source and mask scratch buffers must differ"
    );

    let (before, target_and_after) = scratch.split_at_mut(target_ix);
    let (target_slice, after) = target_and_after.split_at_mut(1);
    let target = &mut target_slice[0];
    let source = scratch_ref_except_target(before, after, target_ix, source_ix);
    let mask = scratch_ref_except_target(before, after, target_ix, mask_ix);
    (target, source, mask)
}

fn scratch_ref_except_target<'a>(
    before: &'a [CubeBuffer<u32>],
    after: &'a [CubeBuffer<u32>],
    target_ix: usize,
    ix: usize,
) -> &'a CubeBuffer<u32> {
    if ix < target_ix {
        &before[ix]
    } else {
        &after[ix - target_ix - 1]
    }
}

impl WgpuRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
    }
}

struct DrawSdfUpload {
    kinds: Vec<u32>,
    x0: Vec<f32>,
    y0: Vec<f32>,
    x1: Vec<f32>,
    y1: Vec<f32>,
    r0: Vec<f32>,
    r1: Vec<f32>,
    r2: Vec<f32>,
    r3: Vec<f32>,
    stroke_top: Vec<f32>,
    stroke_right: Vec<f32>,
    stroke_bottom: Vec<f32>,
    stroke_left: Vec<f32>,
}

impl DrawSdfUpload {
    fn new(draws: &[DrawRecord]) -> Self {
        let mut upload = Self {
            kinds: Vec::with_capacity(draws.len()),
            x0: Vec::with_capacity(draws.len()),
            y0: Vec::with_capacity(draws.len()),
            x1: Vec::with_capacity(draws.len()),
            y1: Vec::with_capacity(draws.len()),
            r0: Vec::with_capacity(draws.len()),
            r1: Vec::with_capacity(draws.len()),
            r2: Vec::with_capacity(draws.len()),
            r3: Vec::with_capacity(draws.len()),
            stroke_top: Vec::with_capacity(draws.len()),
            stroke_right: Vec::with_capacity(draws.len()),
            stroke_bottom: Vec::with_capacity(draws.len()),
            stroke_left: Vec::with_capacity(draws.len()),
        };

        for draw in draws {
            match draw.sdf {
                Some(Sdf::Rect(rect)) => {
                    let (x0, y0, x1, y1) = rect.axis_bounds();
                    upload.push(
                        CUBE_SDF_RECT,
                        [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                        [
                            rect.radius.top_left,
                            rect.radius.top_right,
                            rect.radius.bottom_left,
                            rect.radius.bottom_right,
                        ],
                        [0.0; 4],
                    );
                }
                Some(Sdf::RectStroke(stroke)) => {
                    let (x0, y0, x1, y1) = stroke.rect.axis_bounds();
                    let half = stroke.widths.half();
                    upload.push(
                        CUBE_SDF_RECT_STROKE,
                        [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                        [
                            stroke.rect.radius.top_left,
                            stroke.rect.radius.top_right,
                            stroke.rect.radius.bottom_left,
                            stroke.rect.radius.bottom_right,
                        ],
                        [half.top, half.right, half.bottom, half.left],
                    );
                }
                Some(Sdf::Circle(circle)) => {
                    upload.push(
                        CUBE_SDF_CIRCLE,
                        [
                            circle.center.x as f32,
                            circle.center.y as f32,
                            circle.radius,
                            0.0,
                        ],
                        [0.0; 4],
                        [0.0; 4],
                    );
                }
                Some(Sdf::CircleStroke(stroke)) => {
                    upload.push(
                        CUBE_SDF_CIRCLE_STROKE,
                        [
                            stroke.circle.center.x as f32,
                            stroke.circle.center.y as f32,
                            stroke.circle.radius,
                            0.0,
                        ],
                        [0.0; 4],
                        [stroke.half_width; 4],
                    );
                }
                None => {
                    upload.push(CUBE_SDF_NONE, [0.0; 4], [0.0; 4], [0.0; 4]);
                }
            }
        }

        upload
    }

    fn push(&mut self, kind: u32, xy: [f32; 4], radii: [f32; 4], stroke_widths: [f32; 4]) {
        self.kinds.push(kind);
        self.x0.push(xy[0]);
        self.y0.push(xy[1]);
        self.x1.push(xy[2]);
        self.y1.push(xy[3]);
        self.r0.push(radii[0]);
        self.r1.push(radii[1]);
        self.r2.push(radii[2]);
        self.r3.push(radii[3]);
        self.stroke_top.push(stroke_widths[0]);
        self.stroke_right.push(stroke_widths[1]);
        self.stroke_bottom.push(stroke_widths[2]);
        self.stroke_left.push(stroke_widths[3]);
    }
}

pub(crate) struct SceneBuffers {
    pub(crate) line_path_ids: CubeBuffer<u32>,
    pub(crate) line_p0x: CubeBuffer<f32>,
    pub(crate) line_p0y: CubeBuffer<f32>,
    pub(crate) line_p1x: CubeBuffer<f32>,
    pub(crate) line_p1y: CubeBuffer<f32>,
    pub(crate) draw_path_ids: CubeBuffer<u32>,
    pub(crate) draw_tags: CubeBuffer<u32>,
    pub(crate) draw_fill_rules: CubeBuffer<u32>,
    pub(crate) draw_solid_rects: CubeBuffer<u32>,
    pub(crate) draw_solid_color_fast_paths: CubeBuffer<u32>,
    pub(crate) draw_brush_colors: CubeBuffer<u32>,
    pub(crate) draw_pixel_x0: CubeBuffer<i32>,
    pub(crate) draw_pixel_y0: CubeBuffer<i32>,
    pub(crate) draw_pixel_x1: CubeBuffer<i32>,
    pub(crate) draw_pixel_y1: CubeBuffer<i32>,
    pub(crate) draw_sdf_kinds: CubeBuffer<u32>,
    pub(crate) draw_sdf_x0: CubeBuffer<f32>,
    pub(crate) draw_sdf_y0: CubeBuffer<f32>,
    pub(crate) draw_sdf_x1: CubeBuffer<f32>,
    pub(crate) draw_sdf_y1: CubeBuffer<f32>,
    pub(crate) draw_sdf_r0: CubeBuffer<f32>,
    pub(crate) draw_sdf_r1: CubeBuffer<f32>,
    pub(crate) draw_sdf_r2: CubeBuffer<f32>,
    pub(crate) draw_sdf_r3: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_top: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_right: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_bottom: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_left: CubeBuffer<f32>,
    pub(crate) backdrop_data_offsets: CubeBuffer<u32>,
    pub(crate) backdrop_data_lens: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x1: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y1: CubeBuffer<u32>,
    pub(crate) backdrop_segment_starts: CubeBuffer<u32>,
    pub(crate) backdrop_segment_capacities: CubeBuffer<u32>,
    pub(crate) scan_chunk_path_ids: CubeBuffer<u32>,
    pub(crate) scan_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) scan_chunk_segment_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_lens: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_ends: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_lens: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_starts: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_ends: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_tags: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_draws: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_payloads: CubeBuffer<u32>,
}

impl SceneBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            line_path_ids: CubeBuffer::new(client, 0),
            line_p0x: CubeBuffer::new(client, 0),
            line_p0y: CubeBuffer::new(client, 0),
            line_p1x: CubeBuffer::new(client, 0),
            line_p1y: CubeBuffer::new(client, 0),
            draw_path_ids: CubeBuffer::new(client, 0),
            draw_tags: CubeBuffer::new(client, 0),
            draw_fill_rules: CubeBuffer::new(client, 0),
            draw_solid_rects: CubeBuffer::new(client, 0),
            draw_solid_color_fast_paths: CubeBuffer::new(client, 0),
            draw_brush_colors: CubeBuffer::new(client, 0),
            draw_pixel_x0: CubeBuffer::new(client, 0),
            draw_pixel_y0: CubeBuffer::new(client, 0),
            draw_pixel_x1: CubeBuffer::new(client, 0),
            draw_pixel_y1: CubeBuffer::new(client, 0),
            draw_sdf_kinds: CubeBuffer::new(client, 0),
            draw_sdf_x0: CubeBuffer::new(client, 0),
            draw_sdf_y0: CubeBuffer::new(client, 0),
            draw_sdf_x1: CubeBuffer::new(client, 0),
            draw_sdf_y1: CubeBuffer::new(client, 0),
            draw_sdf_r0: CubeBuffer::new(client, 0),
            draw_sdf_r1: CubeBuffer::new(client, 0),
            draw_sdf_r2: CubeBuffer::new(client, 0),
            draw_sdf_r3: CubeBuffer::new(client, 0),
            draw_sdf_stroke_top: CubeBuffer::new(client, 0),
            draw_sdf_stroke_right: CubeBuffer::new(client, 0),
            draw_sdf_stroke_bottom: CubeBuffer::new(client, 0),
            draw_sdf_stroke_left: CubeBuffer::new(client, 0),
            backdrop_data_offsets: CubeBuffer::new(client, 0),
            backdrop_data_lens: CubeBuffer::new(client, 0),
            backdrop_tile_x0: CubeBuffer::new(client, 0),
            backdrop_tile_y0: CubeBuffer::new(client, 0),
            backdrop_tile_x1: CubeBuffer::new(client, 0),
            backdrop_tile_y1: CubeBuffer::new(client, 0),
            backdrop_segment_starts: CubeBuffer::new(client, 0),
            backdrop_segment_capacities: CubeBuffer::new(client, 0),
            scan_chunk_path_ids: CubeBuffer::new(client, 0),
            scan_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            scan_chunk_segment_starts: CubeBuffer::new(client, 0),
            scan_chunk_lens: CubeBuffer::new(client, 0),
            scan_chunk_range_starts: CubeBuffer::new(client, 0),
            scan_chunk_range_ends: CubeBuffer::new(client, 0),
            cumsum_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_lens: CubeBuffer::new(client, 0),
            cumsum_row_chunk_starts: CubeBuffer::new(client, 0),
            cumsum_row_chunk_ends: CubeBuffer::new(client, 0),
            plan_layer_stack_tags: CubeBuffer::new(client, 0),
            plan_layer_stack_draws: CubeBuffer::new(client, 0),
            plan_layer_stack_payloads: CubeBuffer::new(client, 0),
        }
    }

    fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        scene: &Scene,
        plan: &ExecPlan,
    ) {
        let (scan_chunks, scan_chunk_ranges) = build_scan_chunks(scene);
        let cumsum_plan = build_cumsum_plan(scene);
        self.upload_lines(client, &scene.lines);
        self.upload_draws(client, &scene.draw_records);
        self.upload_backdrops(client, &scene.bd_records);
        self.upload_plan_layer_stack(client, &plan.layer_stack_data);

        self.scan_chunk_path_ids.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.path_id)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_backdrop_offsets.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.backdrop_offset)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_segment_starts.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.segment_start)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_lens.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.len)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_range_starts.replace(
            client,
            &scan_chunk_ranges
                .iter()
                .map(|range| range.start)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_range_ends.replace(
            client,
            &scan_chunk_ranges
                .iter()
                .map(|range| range.end)
                .collect::<Vec<_>>(),
        );
        self.cumsum_chunk_backdrop_offsets
            .replace(client, &cumsum_plan.chunk_backdrop_offsets);
        self.cumsum_chunk_lens
            .replace(client, &cumsum_plan.chunk_lens);
        self.cumsum_row_chunk_starts
            .replace(client, &cumsum_plan.row_chunk_starts);
        self.cumsum_row_chunk_ends
            .replace(client, &cumsum_plan.row_chunk_ends);
    }

    fn upload_plan_layer_stack<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        layer_stack: &[LayerStackEntry],
    ) {
        self.plan_layer_stack_tags.replace(
            client,
            &layer_stack
                .iter()
                .map(|entry| match entry {
                    LayerStackEntry::Clip { .. } => CUBE_LAYER_CLIP,
                    LayerStackEntry::Opacity { .. } => CUBE_LAYER_OPACITY,
                    LayerStackEntry::Blend { .. } => CUBE_LAYER_BLEND,
                })
                .collect::<Vec<_>>(),
        );
        self.plan_layer_stack_draws.replace(
            client,
            &layer_stack
                .iter()
                .map(|entry| match *entry {
                    LayerStackEntry::Clip { draw }
                    | LayerStackEntry::Opacity { draw, .. }
                    | LayerStackEntry::Blend { draw, .. } => draw,
                })
                .collect::<Vec<_>>(),
        );
        self.plan_layer_stack_payloads.replace(
            client,
            &layer_stack
                .iter()
                .map(|entry| encode_layer_payload(*entry))
                .collect::<Vec<_>>(),
        );
    }

    fn upload_lines<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lines: &[Line],
    ) {
        self.line_path_ids.replace(
            client,
            &lines.iter().map(|line| line.path_id).collect::<Vec<_>>(),
        );
        self.line_p0x.replace(
            client,
            &lines.iter().map(|line| line.p0[0]).collect::<Vec<_>>(),
        );
        self.line_p0y.replace(
            client,
            &lines.iter().map(|line| line.p0[1]).collect::<Vec<_>>(),
        );
        self.line_p1x.replace(
            client,
            &lines.iter().map(|line| line.p1[0]).collect::<Vec<_>>(),
        );
        self.line_p1y.replace(
            client,
            &lines.iter().map(|line| line.p1[1]).collect::<Vec<_>>(),
        );
    }

    fn upload_draws<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        draws: &[DrawRecord],
    ) {
        self.draw_path_ids.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.path_id.unwrap_or(u32::MAX))
                .collect::<Vec<_>>(),
        );
        self.draw_tags.replace(
            client,
            &draws
                .iter()
                .map(|draw| match draw.tag {
                    DrawTag::Brush => CUBE_DRAW_BRUSH,
                    DrawTag::Clip => CUBE_DRAW_CLIP,
                    DrawTag::Opacity => CUBE_DRAW_OPACITY,
                    DrawTag::Blend => CUBE_DRAW_BLEND,
                })
                .collect::<Vec<_>>(),
        );
        self.draw_fill_rules.replace(
            client,
            &draws
                .iter()
                .map(|draw| match draw.fill_rule {
                    FillRule::NonZero => 0,
                    FillRule::EvenOdd => 1,
                })
                .collect::<Vec<_>>(),
        );
        self.draw_solid_rects.replace(
            client,
            &draws
                .iter()
                .map(|draw| u32::from(draw.solid_rect))
                .collect::<Vec<_>>(),
        );
        self.draw_solid_color_fast_paths.replace(
            client,
            &draws
                .iter()
                .map(|draw| u32::from(draw.solid_rect && draw.brush.solid_color().is_some()))
                .collect::<Vec<_>>(),
        );
        self.draw_brush_colors.replace(
            client,
            &draws
                .iter()
                .map(|draw| {
                    draw.brush
                        .solid_color()
                        .map(|color| premul_f32_to_u32(color.premultiply().components))
                        .unwrap_or(0)
                })
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_x0.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.x0)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_y0.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.y0)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_x1.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.x1)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_y1.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.y1)
                .collect::<Vec<_>>(),
        );
        let sdf_upload = DrawSdfUpload::new(draws);
        self.draw_sdf_kinds.replace(client, &sdf_upload.kinds);
        self.draw_sdf_x0.replace(client, &sdf_upload.x0);
        self.draw_sdf_y0.replace(client, &sdf_upload.y0);
        self.draw_sdf_x1.replace(client, &sdf_upload.x1);
        self.draw_sdf_y1.replace(client, &sdf_upload.y1);
        self.draw_sdf_r0.replace(client, &sdf_upload.r0);
        self.draw_sdf_r1.replace(client, &sdf_upload.r1);
        self.draw_sdf_r2.replace(client, &sdf_upload.r2);
        self.draw_sdf_r3.replace(client, &sdf_upload.r3);
        self.draw_sdf_stroke_top
            .replace(client, &sdf_upload.stroke_top);
        self.draw_sdf_stroke_right
            .replace(client, &sdf_upload.stroke_right);
        self.draw_sdf_stroke_bottom
            .replace(client, &sdf_upload.stroke_bottom);
        self.draw_sdf_stroke_left
            .replace(client, &sdf_upload.stroke_left);
    }

    fn upload_backdrops<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        records: &[BackdropRecord],
    ) {
        self.backdrop_data_offsets.replace(
            client,
            &records
                .iter()
                .map(|record| record.data_offset)
                .collect::<Vec<_>>(),
        );
        self.backdrop_data_lens.replace(
            client,
            &records
                .iter()
                .map(|record| record.data_len)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_x0.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_x0)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_y0.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_y0)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_x1.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_x1)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_y1.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_y1)
                .collect::<Vec<_>>(),
        );
        self.backdrop_segment_starts.replace(
            client,
            &records
                .iter()
                .map(|record| record.segment_start)
                .collect::<Vec<_>>(),
        );
        self.backdrop_segment_capacities.replace(
            client,
            &records
                .iter()
                .map(|record| record.segment_capacity)
                .collect::<Vec<_>>(),
        );
    }
}

pub(crate) struct ScanBuffers {
    pub(crate) backdrops: CubeBuffer<i32>,
    pub(crate) tile_segment_range_starts: CubeBuffer<u32>,
    pub(crate) tile_segment_range_ends: CubeBuffer<u32>,
    pub(crate) segment_p0x: CubeBuffer<f32>,
    pub(crate) segment_p0y: CubeBuffer<f32>,
    pub(crate) segment_p1x: CubeBuffer<f32>,
    pub(crate) segment_p1y: CubeBuffer<f32>,
    pub(crate) segment_y_edge: CubeBuffer<f32>,
    pub(crate) segment_tile_counts: CubeBuffer<u32>,
    pub(crate) segment_tile_cursors: CubeBuffer<u32>,
    pub(crate) segment_bumps: CubeBuffer<u32>,
    pub(crate) chunk_totals: CubeBuffer<u32>,
    pub(crate) chunk_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_totals: CubeBuffer<i32>,
    pub(crate) cumsum_chunk_offsets: CubeBuffer<i32>,
}

impl ScanBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            backdrops: CubeBuffer::new(client, 0),
            tile_segment_range_starts: CubeBuffer::new(client, 0),
            tile_segment_range_ends: CubeBuffer::new(client, 0),
            segment_p0x: CubeBuffer::new(client, 0),
            segment_p0y: CubeBuffer::new(client, 0),
            segment_p1x: CubeBuffer::new(client, 0),
            segment_p1y: CubeBuffer::new(client, 0),
            segment_y_edge: CubeBuffer::new(client, 0),
            segment_tile_counts: CubeBuffer::new(client, 0),
            segment_tile_cursors: CubeBuffer::new(client, 0),
            segment_bumps: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_totals: CubeBuffer::new(client, 0),
            cumsum_chunk_offsets: CubeBuffer::new(client, 0),
        }
    }

    fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.backdrops.resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_starts
            .resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_ends
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_p0x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p0y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_y_edge
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_tile_counts
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_tile_cursors
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_bumps.resize_uninit(client, lengths.path_count);
        self.chunk_totals
            .resize_uninit(client, lengths.scan_chunk_count);
        self.chunk_offsets
            .resize_uninit(client, lengths.scan_chunk_count);
        self.cumsum_chunk_totals
            .resize_uninit(client, lengths.cumsum_chunk_count);
        self.cumsum_chunk_offsets
            .resize_uninit(client, lengths.cumsum_chunk_count);
    }
}

pub(crate) struct CoarseBuffers {
    pub(crate) tile_ptcl_range_starts: CubeBuffer<u32>,
    pub(crate) tile_ptcl_range_ends: CubeBuffer<u32>,
    pub(crate) tile_ptcl_counts: CubeBuffer<u32>,
    pub(crate) chunk_totals: CubeBuffer<u32>,
    pub(crate) chunk_offsets: CubeBuffer<u32>,
    pub(crate) ptcl_tags: CubeBuffer<u32>,
    pub(crate) ptcl_backdrops: CubeBuffer<i32>,
    pub(crate) ptcl_fill_rules: CubeBuffer<u32>,
    pub(crate) ptcl_segment_starts: CubeBuffer<u32>,
    pub(crate) ptcl_segment_ends: CubeBuffer<u32>,
    pub(crate) ptcl_colors: CubeBuffer<u32>,
}

impl CoarseBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            tile_ptcl_range_starts: CubeBuffer::new(client, 0),
            tile_ptcl_range_ends: CubeBuffer::new(client, 0),
            tile_ptcl_counts: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
            ptcl_tags: CubeBuffer::new(client, 0),
            ptcl_backdrops: CubeBuffer::new(client, 0),
            ptcl_fill_rules: CubeBuffer::new(client, 0),
            ptcl_segment_starts: CubeBuffer::new(client, 0),
            ptcl_segment_ends: CubeBuffer::new(client, 0),
            ptcl_colors: CubeBuffer::new(client, 0),
        }
    }

    fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.tile_ptcl_range_starts
            .resize_uninit(client, lengths.tile_count);
        self.tile_ptcl_range_ends
            .resize_uninit(client, lengths.tile_count);
        self.tile_ptcl_counts
            .resize_uninit(client, lengths.tile_count);
        self.chunk_totals
            .resize_uninit(client, lengths.coarse_chunk_count);
        self.chunk_offsets
            .resize_uninit(client, lengths.coarse_chunk_count);
        self.ptcl_tags
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_backdrops
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_fill_rules
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_segment_starts
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_segment_ends
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_colors
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
    }
}

#[cfg(test)]
mod tests;
