use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, Color, Extend};

use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        bounds::Bounds,
        brush::Brush,
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry, ROOT_COMMAND_LIST_ID},
        fill::FillRule,
        image::premul_color_to_rgba8_pack,
        layer::{Layer, filter::Filter, region::Region},
        line::Line,
        pixel::{opacity_f32_to_u8, premul_f32_to_u32},
    },
};

use super::{
    buffer::CubeBuffer,
    pipelines::{
        coarse::{CoarseBatch, CoarsePipeline},
        cumsum::CumsumPipeline,
        filter::{
            FILTER_BRIGHTNESS, FILTER_BRUSH_FOUR_CORNER, FILTER_BRUSH_LINEAR,
            FILTER_BRUSH_PARAM_STRIDE, FILTER_BRUSH_PATTERN, FILTER_BRUSH_RADIAL,
            FILTER_BRUSH_SOLID, FILTER_BRUSH_SWEEP, FILTER_BRUSH_U32_STRIDE, FILTER_CONTRAST,
            FILTER_EXTEND_PAD, FILTER_EXTEND_REFLECT, FILTER_EXTEND_REPEAT, FILTER_GRAYSCALE,
            FILTER_HUE_ROTATE, FILTER_INVERT, FILTER_OPACITY, FILTER_SATURATE, FILTER_SEPIA,
            FilterBrushResources, FilterPipeline,
        },
        fine::{FinePipeline, FineRenderConfig},
        scan::ScanPipeline,
    },
    types::{
        CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_OPACITY, CUBE_LAYER_BLEND,
        CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY, CubeBufferLengths, CubeSceneConfig, build_cumsum_plan,
        build_scan_chunks,
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
    filter_brushes: FilterBrushBuffers,
    target: CubeBuffer<u32>,
    scratch: Vec<CubeBuffer<u32>>,
    scratch_in_use: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CubeRenderTarget {
    Main,
    Scratch(usize),
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
            filter_brushes: FilterBrushBuffers::new(&client),
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
        let filter_brush_upload = FilterBrushUpload::from_plan(&plan);
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.prepare_scratch_buffers(scratch_count);
        self.filter_brushes
            .upload(&self.client, filter_brush_upload);
        self.scene.upload(&self.client, scene, &plan);
        self.scan.prepare_outputs(&self.client, lengths);
        self.coarse.prepare_outputs(&self.client, lengths);
        self.config.replace(
            &self.client,
            &[CubeSceneConfig::new(scene, lengths, self.clear_color)],
        );
        self.plan = Some(plan);
    }

    /// Runs the CubeCL scan stage.
    ///
    /// The algorithm mirrors the CPU scan pipeline but splits it into GPU
    /// passes: clear, per-line counting, chunk-local prefix, per-path chunk
    /// offsets, offset application, and segment emission. This is a real GPU
    /// implementation, not a temporary CPU fallback.
    pub fn scan(&mut self) {
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    /// Runs the CubeCL backdrop row prefix-sum stage.
    ///
    /// This consumes scan's backdrop edge deltas and leaves cumulative backdrop
    /// values in the same GPU buffer for coarse/fine stages. The pass is fully
    /// GPU-resident; readback exists only in tests.
    pub fn cumsum(&mut self) {
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    /// Runs the CubeCL coarse stage over the whole flat draw list.
    ///
    /// The stage is fully GPU-resident: count visible draw particles per tile,
    /// prefix those counts into compact ranges, then emit ordered particles.
    /// Full scene rendering should use `render`, which runs coarse per
    /// execution-plan draw batch with its active layer stack.
    pub fn coarse(&mut self) {
        self.coarse_batch(0, self.lengths.draw_count as u32, 0, 0);
    }

    fn coarse_batch(
        &mut self,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
    ) {
        CoarsePipeline::run(
            &self.client,
            &self.scene,
            &self.scan,
            &mut self.coarse,
            self.lengths,
            CoarseBatch {
                draw_start,
                draw_end,
                layer_stack_start,
                layer_stack_end,
            },
        );
    }

    /// Runs the CubeCL fine stage for the current coarse particle stream.
    ///
    /// The pass clears the target, then renders one workgroup per tile with one
    /// lane per pixel. Use `fine_batch` internally when compositing multiple
    /// execution-plan batches into the same target.
    pub fn fine(&mut self) {
        let config = self.fine_config();
        FinePipeline::run(
            &self.client,
            &self.scan,
            &self.coarse,
            &mut self.target,
            config,
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
                &self.scan,
                &self.coarse,
                &mut self.target,
                config,
            ),
            CubeRenderTarget::Scratch(ix) => FinePipeline::render(
                &self.client,
                &self.scan,
                &self.coarse,
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
        self.prepare_scene(scene);
        self.scan();
        self.cumsum();
        self.clear_target();
        self.execute_prepared_plan();
    }

    pub fn render_flat(&mut self, scene: &Scene) {
        self.render(scene);
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

    pub fn client(&self) -> &::cubecl::client::ComputeClient<R> {
        &self.client
    }

    pub fn runtime_name(&self) -> &'static str {
        R::name(&self.client)
    }

    fn execute_prepared_plan(&mut self) {
        let plan = self
            .plan
            .take()
            .expect("CubeCL execute requires prepare_scene to upload an execution plan first");
        self.execute_plan(&plan);
        self.plan = Some(plan);
    }

    fn execute_plan(&mut self, plan: &ExecPlan) {
        let mut filter_brush_cursor = 0;
        self.execute_ops(
            plan,
            &plan.ops,
            CubeRenderTarget::Main,
            &mut filter_brush_cursor,
        );
    }

    fn execute_ops(
        &mut self,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: CubeRenderTarget,
        filter_brush_cursor: &mut usize,
    ) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => {
                    self.execute_draw_batch(plan, draws.clone(), layer_stack.clone(), target);
                }
                ExecOp::BeginClip { .. }
                | ExecOp::EndClip
                | ExecOp::BeginOpacity { .. }
                | ExecOp::EndOpacity { .. }
                | ExecOp::BeginBlend { .. }
                | ExecOp::EndBlend { .. } => {}
                ExecOp::OffscreenLayer {
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    plan,
                    layer,
                    outer_stack.clone(),
                    children,
                    target,
                    filter_brush_cursor,
                ),
            }
        }
    }

    fn execute_draw_batch(
        &mut self,
        _plan: &ExecPlan,
        draws: std::ops::Range<usize>,
        layer_stack: std::ops::Range<usize>,
        target: CubeRenderTarget,
    ) {
        if draws.start >= draws.end {
            return;
        }
        self.coarse_batch(
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
        );
        self.fine_batch_to(target);
    }

    fn execute_offscreen_layer(
        &mut self,
        plan: &ExecPlan,
        layer: &Layer,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: CubeRenderTarget,
        filter_brush_cursor: &mut usize,
    ) {
        if !outer_stack.is_empty() {
            panic!(
                "CubeCL offscreen layers do not support an outer fused clip/opacity/blend stack yet"
            );
        }

        match layer {
            Layer::Filter {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(plan, children, source, filter_brush_cursor);
                let brush_index = next_filter_brush_index(filter, filter_brush_cursor);
                self.apply_filter(source, bounds, filter, brush_index);
                self.composite_src_over(target, source, bounds);
                self.release_scratch(source);
            }
            Layer::Backdrop {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                let brush_index = next_filter_brush_index(filter, filter_brush_cursor);
                let backdrop = self.acquire_scratch();
                self.clear_buffer(backdrop, 0);
                self.copy_region(target, backdrop, bounds);
                self.apply_filter(backdrop, bounds, filter, brush_index);
                self.composite_src_over(target, backdrop, bounds);
                self.release_scratch(backdrop);

                let content = self.acquire_scratch();
                self.clear_buffer(content, 0);
                self.execute_ops(plan, children, content, filter_brush_cursor);
                self.composite_src_over(target, content, Bounds::canvas(self.size.0, self.size.1));
                self.release_scratch(content);
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
}

fn plan_stack_depths(plan: &ExecPlan) -> (usize, usize) {
    let mut max_clip_depth = 0;
    let mut max_group_depth = 0;
    for op in &plan.ops {
        if let ExecOp::DrawBatch { layer_stack, .. } = op {
            let entries = &plan.layer_stack_data[layer_stack.clone()];
            max_clip_depth = max_clip_depth.max(
                entries
                    .iter()
                    .filter(|entry| matches!(entry, LayerStackEntry::Clip { .. }))
                    .count(),
            );
            max_group_depth = max_group_depth.max(
                entries
                    .iter()
                    .filter(|entry| {
                        matches!(
                            entry,
                            LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. }
                        )
                    })
                    .count(),
            );
        }
    }
    (max_clip_depth, max_group_depth)
}

fn required_scratch_count(plan: &ExecPlan) -> usize {
    max_scratch_for_ops(&plan.ops, 0)
}

fn max_scratch_for_ops(ops: &[ExecOp], held: usize) -> usize {
    let mut max_count = held;
    for op in ops {
        if let ExecOp::OffscreenLayer {
            layer, children, ..
        } = op
        {
            match layer {
                Layer::Filter { filter, .. } => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + filter_scratch_extra(filter));
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                Layer::Backdrop { filter, .. } => {
                    let backdrop_held = held + 1;
                    max_count = max_count.max(backdrop_held + filter_scratch_extra(filter));
                    max_count = max_count.max(max_scratch_for_ops(children, backdrop_held));
                }
                _ => {}
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

struct FilterBrushBuffers {
    data: CubeBuffer<u32>,
    params: CubeBuffer<f32>,
    payloads: CubeBuffer<u32>,
}

impl FilterBrushBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            data: CubeBuffer::new(client, 0),
            params: CubeBuffer::new(client, 0),
            payloads: CubeBuffer::new(client, 0),
        }
    }

    fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        upload: FilterBrushUpload,
    ) {
        self.data.replace(client, &upload.data);
        self.params.replace(client, &upload.params);
        self.payloads.replace(client, &upload.payloads);
    }

    fn resources(&self) -> FilterBrushResources<'_> {
        FilterBrushResources {
            data: &self.data,
            params: &self.params,
            payloads: &self.payloads,
        }
    }
}

#[derive(Default)]
struct FilterBrushUpload {
    data: Vec<u32>,
    params: Vec<f32>,
    payloads: Vec<u32>,
}

impl FilterBrushUpload {
    fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_brushes_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_brush(&mut self, brush: &Brush) {
        let mut params = [0.0; FILTER_BRUSH_PARAM_STRIDE];
        let mut kind = FILTER_BRUSH_SOLID;
        let mut extend = FILTER_EXTEND_PAD;
        let mut color = 0;
        let mut image_width = 0;
        let mut image_height = 0;
        let mut opacity = 255;
        let mut payload_offset = 0;
        let mut payload_len = 0;

        match brush {
            Brush::Solid(value) => {
                color = premul_color_to_rgba8_pack(*value);
            }
            Brush::Linear(gradient) => {
                kind = FILTER_BRUSH_LINEAR;
                extend = encode_filter_extend(gradient.extend);
                params[0] = gradient.start[0];
                params[1] = gradient.start[1];
                params[2] = gradient.end[0];
                params[3] = gradient.end[1];
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::Radial(gradient) => {
                kind = FILTER_BRUSH_RADIAL;
                extend = encode_filter_extend(gradient.extend);
                params[0] = gradient.start_center[0];
                params[1] = gradient.start_center[1];
                params[2] = gradient.end_center[0];
                params[3] = gradient.end_center[1];
                params[4] = gradient.start_radius;
                params[5] = gradient.end_radius;
                params[6..12].copy_from_slice(&gradient.transform);
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::Sweep(gradient) => {
                kind = FILTER_BRUSH_SWEEP;
                extend = encode_filter_extend(gradient.extend);
                params[0] = gradient.center[0];
                params[1] = gradient.center[1];
                params[2] = gradient.start_angle;
                params[3] = gradient.end_angle;
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::FourCorner(gradient) => {
                kind = FILTER_BRUSH_FOUR_CORNER;
                params[0..4].copy_from_slice(&gradient.bounds);
                (payload_offset, payload_len) = self.push_payload(&gradient.colors);
            }
            Brush::Pattern(pattern) => {
                kind = FILTER_BRUSH_PATTERN;
                params[0..6].copy_from_slice(&pattern.transform);
                image_width = pattern.image.width;
                image_height = pattern.image.height;
                opacity = pattern.opacity as u32;
                (payload_offset, payload_len) = self.push_payload(&pattern.image.pixels);
            }
        }

        self.data.extend_from_slice(&[
            kind,
            extend,
            payload_offset,
            payload_len,
            color,
            image_width,
            image_height,
            opacity,
        ]);
        debug_assert_eq!(self.data.len() % FILTER_BRUSH_U32_STRIDE, 0);
        self.params.extend_from_slice(&params);
    }

    fn push_payload(&mut self, payload: &[u32]) -> (u32, u32) {
        let offset = self.payloads.len() as u32;
        self.payloads.extend_from_slice(payload);
        (offset, payload.len() as u32)
    }
}

fn collect_filter_brushes_for_ops(ops: &[ExecOp], upload: &mut FilterBrushUpload) {
    for op in ops {
        if let ExecOp::OffscreenLayer {
            layer, children, ..
        } = op
        {
            match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_brushes_for_ops(children, upload);
                    collect_filter_brush(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_brush(filter, upload);
                    collect_filter_brushes_for_ops(children, upload);
                }
                _ => {}
            }
        }
    }
}

fn collect_filter_brush(filter: &Filter, upload: &mut FilterBrushUpload) {
    if let Filter::DropShadow { brush, .. } = filter {
        upload.push_brush(brush);
    }
}

fn encode_filter_extend(extend: Extend) -> u32 {
    match extend {
        Extend::Pad => FILTER_EXTEND_PAD,
        Extend::Repeat => FILTER_EXTEND_REPEAT,
        Extend::Reflect => FILTER_EXTEND_REFLECT,
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
        Region::Path { .. } => {
            panic!("CubeCL offscreen color-filter stage does not support path sample regions yet")
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

impl WgpuRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
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
mod tests {
    use std::sync::Arc;

    use peniko::{
        Color, Compose, Gradient, Mix,
        kurbo::{Affine, Rect, Shape},
    };

    use super::CubeBufferLengths;
    use super::WgpuRenderer;
    use crate::cubecl::pipelines::coarse::TILE_WORKGROUP_SIZE;
    use crate::cubecl::types::{
        CUBE_PTCL_BEGIN_BLEND, CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_BEGIN_OPACITY, CUBE_PTCL_COLOR,
        CUBE_PTCL_END, CUBE_PTCL_END_BLEND, CUBE_PTCL_END_CLIP, CUBE_PTCL_END_OPACITY,
        CUBE_PTCL_FILL, CUMSUM_CHUNK_SIZE,
    };
    use crate::shared::brush::{Brush, IDENTITY_TRANSFORM, PatternBrush};
    use crate::shared::execution::ExecOp;
    use crate::shared::image::{Image, rgba8_pack, unpack_rgba8};
    use crate::shared::layer::{filter::Filter, region::Region};
    use crate::shared::pixel::premul_f32_to_u32;
    use crate::{FillRule, Radius, Scene};

    #[test]
    fn buffer_lengths_keep_empty_scene_allocations_zero_sized_except_target() {
        let scene = Scene::new(33, 17);
        let lengths = CubeBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, 0);
        assert_eq!(lengths.path_count, 0);
        assert_eq!(lengths.backdrop_len, 0);
        assert_eq!(lengths.segment_capacity, 0);
        assert_eq!(lengths.scan_chunk_count, 0);
        assert_eq!(lengths.cumsum_chunk_count, 0);
        assert_eq!(lengths.cumsum_row_count, 0);
        assert_eq!(lengths.coarse_chunk_count, 1);
        assert_eq!(lengths.coarse_ptcl_capacity, 6);
        assert_eq!(lengths.tiles_width, 3);
        assert_eq!(lengths.tiles_height, 2);
        assert_eq!(lengths.tile_count, 6);
        assert_eq!(lengths.image_pixels, 33 * 17);
    }

    #[test]
    fn buffer_lengths_match_scene_preallocated_path_storage() {
        let mut scene = Scene::new(64, 48);
        scene.push_path(
            Rect::new(8.0, 8.0, 40.0, 32.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let lengths = CubeBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, scene.lines.len());
        assert_eq!(lengths.path_count, scene.path_records.len());
        assert_eq!(lengths.draw_count, scene.draw_records.len());
        assert_eq!(lengths.backdrop_record_count, scene.bd_records.len());
        assert_eq!(lengths.backdrop_len, scene.backdrop_pool_capacity as usize);
        assert_eq!(lengths.segment_capacity, scene.tile_cnt as usize);
        assert_eq!(lengths.scan_chunk_count, 1);
        assert_eq!(lengths.cumsum_chunk_count, 2);
        assert_eq!(lengths.cumsum_row_count, 2);
        assert_eq!(lengths.coarse_chunk_count, 1);
        assert_eq!(lengths.coarse_ptcl_capacity, 18);
        assert_eq!(lengths.tiles_width, 4);
        assert_eq!(lengths.tiles_height, 3);
        assert_eq!(lengths.tile_count, 4 * 3);
        assert_eq!(lengths.image_pixels, 64 * 48);
    }

    #[test]
    fn scan_wgpu_emits_one_tile_vertical_line_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_path(
            peniko::kurbo::Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        renderer.scan();

        let ranges_start = renderer
            .scan
            .tile_segment_range_starts
            .read(renderer.client());
        let ranges_end = renderer
            .scan
            .tile_segment_range_ends
            .read(renderer.client());
        let segment_bumps = renderer.scan.segment_bumps.read(renderer.client());
        let backdrops = renderer.scan.backdrops.read(renderer.client());
        let p0x = renderer.scan.segment_p0x.read(renderer.client());
        let p0y = renderer.scan.segment_p0y.read(renderer.client());
        let p1x = renderer.scan.segment_p1x.read(renderer.client());
        let p1y = renderer.scan.segment_p1y.read(renderer.client());

        assert_eq!(backdrops, vec![0]);
        assert_eq!(segment_bumps, vec![1]);
        assert_eq!(ranges_start, vec![0]);
        assert_eq!(ranges_end, vec![1]);
        assert!((p0x[0] - 4.0).abs() < 1e-3);
        assert!((p1x[0] - 4.0).abs() < 1e-3);
        assert!((p0y[0] - 0.0).abs() < 1e-6);
        assert!((p1y[0] - 16.0).abs() < 1e-6);
    }

    #[test]
    fn cumsum_wgpu_scans_backdrop_rows_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
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

        let mut renderer = WgpuRenderer::new_default_device(48, 32, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer
            .scan
            .backdrops
            .replace(&client, &[1, -1, 2, 3, 0, -2]);
        renderer.cumsum();

        assert_eq!(
            renderer.scan.backdrops.read(renderer.client()),
            vec![1, 0, 2, 3, 3, 1]
        );
    }

    #[test]
    fn cumsum_wgpu_carries_across_chunks_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let row_tiles = CUMSUM_CHUNK_SIZE + 3;
        let mut scene = Scene::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2);
        scene.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from(row_tiles * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE * 2),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut deltas = Vec::with_capacity((row_tiles * 2) as usize);
        let mut expected = Vec::with_capacity((row_tiles * 2) as usize);
        for row in 0..2 {
            let mut carry = 0;
            for x in 0..row_tiles {
                let value = if row == 0 {
                    1
                } else if x % 2 == 0 {
                    2
                } else {
                    -1
                };
                carry += value;
                deltas.push(value);
                expected.push(carry);
            }
        }

        let mut renderer = WgpuRenderer::new_default_device(
            row_tiles * crate::TILE_SIZE,
            crate::TILE_SIZE * 2,
            Color::TRANSPARENT,
        );
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer.scan.backdrops.replace(&client, &deltas);
        renderer.cumsum();

        assert_eq!(renderer.scan.backdrops.read(renderer.client()), expected);
    }

    #[test]
    fn coarse_wgpu_emits_compact_solid_color_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let blue = Color::from_rgb8(0, 0, 255);
        let mut scene = Scene::new(32, 16);
        scene.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), red, FillRule::NonZero);
        scene.push_rect(Rect::new(16.0, 0.0, 32.0, 16.0), blue, FillRule::NonZero);

        let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        assert_eq!(
            renderer.scene.draw_solid_rects.read(renderer.client()),
            vec![1, 1]
        );
        assert_eq!(
            renderer
                .scene
                .draw_solid_color_fast_paths
                .read(renderer.client()),
            vec![1, 1]
        );
        let client = renderer.client.clone();
        renderer.scan.backdrops.replace(&client, &[1, 1, 1]);
        renderer
            .scan
            .tile_segment_range_starts
            .replace(&client, &[0, 0, 0]);
        renderer
            .scan
            .tile_segment_range_ends
            .replace(&client, &[0, 0, 0]);
        renderer.coarse();

        assert_eq!(
            renderer
                .coarse
                .tile_ptcl_range_starts
                .read(renderer.client()),
            vec![0, 2]
        );
        assert_eq!(
            renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
            vec![2, 5]
        );
        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            vec![
                CUBE_PTCL_COLOR,
                CUBE_PTCL_END,
                CUBE_PTCL_COLOR,
                CUBE_PTCL_COLOR,
                CUBE_PTCL_END
            ]
        );
        assert_eq!(
            renderer.coarse.ptcl_colors.read(renderer.client()),
            vec![
                premul_f32_to_u32(red.premultiply().components),
                0,
                premul_f32_to_u32(red.premultiply().components),
                premul_f32_to_u32(blue.premultiply().components),
                0
            ]
        );
    }

    #[test]
    fn coarse_wgpu_keeps_particle_order_across_workgroup_draw_chunks_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let draw_count = TILE_WORKGROUP_SIZE as usize + 3;
        let colors = (0..draw_count)
            .map(|i| {
                Color::from_rgb8(
                    (i % 251) as u8,
                    ((i * 37) % 251) as u8,
                    ((i * 73) % 251) as u8,
                )
            })
            .collect::<Vec<_>>();
        let mut scene = Scene::new(16, 16);
        for color in &colors {
            scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), *color, FillRule::NonZero);
        }

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        assert_eq!(renderer.lengths.coarse_ptcl_capacity, draw_count + 1);

        let client = renderer.client.clone();
        renderer
            .scan
            .backdrops
            .replace(&client, &vec![1; draw_count]);
        renderer
            .scan
            .tile_segment_range_starts
            .replace(&client, &vec![0; draw_count]);
        renderer
            .scan
            .tile_segment_range_ends
            .replace(&client, &vec![0; draw_count]);
        renderer.coarse();

        let mut expected_tags = vec![CUBE_PTCL_COLOR; draw_count];
        expected_tags.push(CUBE_PTCL_END);
        let mut expected_colors = colors
            .iter()
            .map(|color| premul_f32_to_u32(color.premultiply().components))
            .collect::<Vec<_>>();
        expected_colors.push(0);

        assert_eq!(
            renderer
                .coarse
                .tile_ptcl_range_starts
                .read(renderer.client()),
            vec![0]
        );
        assert_eq!(
            renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
            vec![draw_count as u32 + 1]
        );
        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            expected_tags
        );
        assert_eq!(
            renderer.coarse.ptcl_colors.read(renderer.client()),
            expected_colors
        );
    }

    #[test]
    fn coarse_wgpu_keeps_segment_ranges_for_fill_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_path(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::EvenOdd,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer.scan.backdrops.replace(&client, &[0]);
        renderer
            .scan
            .tile_segment_range_starts
            .replace(&client, &[2]);
        renderer.scan.tile_segment_range_ends.replace(&client, &[5]);
        renderer.coarse();

        assert_eq!(
            renderer
                .coarse
                .tile_ptcl_range_starts
                .read(renderer.client()),
            vec![0]
        );
        assert_eq!(
            renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
            vec![2]
        );
        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            vec![CUBE_PTCL_FILL, CUBE_PTCL_END]
        );
        assert_eq!(
            renderer.coarse.ptcl_segment_starts.read(renderer.client()),
            vec![2, 0]
        );
        assert_eq!(
            renderer.coarse.ptcl_segment_ends.read(renderer.client()),
            vec![5, 0]
        );
        assert_eq!(
            renderer.coarse.ptcl_fill_rules.read(renderer.client()),
            vec![1, 0]
        );
    }

    #[test]
    fn coarse_wgpu_emits_clip_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer.scan.backdrops.replace(&client, &[1]);
        renderer
            .scan
            .tile_segment_range_starts
            .replace(&client, &[0]);
        renderer.scan.tile_segment_range_ends.replace(&client, &[0]);
        renderer.coarse();

        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            vec![CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_END, 0]
        );
        assert_eq!(
            renderer.coarse.ptcl_backdrops.read(renderer.client()),
            vec![1, 0, 0]
        );
    }

    #[test]
    fn coarse_wgpu_wraps_draw_batch_with_active_clip_stack_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        renderer.scan();
        renderer.cumsum();

        let plan = renderer.plan.as_ref().unwrap();
        let ExecOp::DrawBatch { draws, layer_stack } = &plan.ops[1] else {
            panic!("expected clipped draw batch");
        };
        let draws = draws.clone();
        let layer_stack = layer_stack.clone();
        renderer.coarse_batch(
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
        );

        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            vec![
                CUBE_PTCL_BEGIN_CLIP,
                CUBE_PTCL_COLOR,
                CUBE_PTCL_END_CLIP,
                CUBE_PTCL_END
            ]
        );
        assert_eq!(
            renderer.coarse.ptcl_colors.read(renderer.client()),
            vec![0, premul_f32_to_u32(red.premultiply().components), 0, 0]
        );
    }

    #[test]
    fn coarse_wgpu_wraps_draw_batch_with_opacity_and_blend_stack_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_opacity_layer(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        scene.push_blend_layer(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        renderer.scan();
        renderer.cumsum();

        let plan = renderer.plan.as_ref().unwrap();
        let (draws, layer_stack) = plan
            .ops
            .iter()
            .find_map(|op| match op {
                ExecOp::DrawBatch { draws, layer_stack }
                    if layer_stack.end - layer_stack.start == 2 =>
                {
                    Some((draws.clone(), layer_stack.clone()))
                }
                _ => None,
            })
            .expect("expected opacity+blend draw batch");
        renderer.coarse_batch(
            draws.start as u32,
            draws.end as u32,
            layer_stack.start as u32,
            layer_stack.end as u32,
        );

        assert_eq!(
            renderer.coarse.ptcl_tags.read(renderer.client()),
            vec![
                CUBE_PTCL_BEGIN_OPACITY,
                CUBE_PTCL_BEGIN_BLEND,
                CUBE_PTCL_COLOR,
                CUBE_PTCL_END_BLEND,
                CUBE_PTCL_END_OPACITY,
                CUBE_PTCL_END
            ]
        );
        assert_eq!(
            renderer.coarse.ptcl_colors.read(renderer.client()),
            vec![
                128,
                Mix::Multiply as u32 | ((Compose::SrcOver as u32) << 8),
                premul_f32_to_u32(red.premultiply().components),
                0,
                0,
                0
            ]
        );
    }

    #[test]
    fn fine_wgpu_renders_solid_color_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let blue = Color::from_rgb8(0, 0, 255);
        let mut scene = Scene::new(32, 16);
        scene.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), red, FillRule::NonZero);
        scene.push_rect(Rect::new(16.0, 0.0, 32.0, 16.0), blue, FillRule::NonZero);

        let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[0], premul_f32_to_u32(red.premultiply().components));
        assert_eq!(target[15], premul_f32_to_u32(red.premultiply().components));
        assert_eq!(target[16], premul_f32_to_u32(blue.premultiply().components));
        assert_eq!(target[31], premul_f32_to_u32(blue.premultiply().components));
    }

    #[test]
    fn fine_wgpu_rasterizes_fill_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_path(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            red,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(
            target[8 * 16 + 8],
            premul_f32_to_u32(red.premultiply().components)
        );
    }

    #[test]
    fn fine_wgpu_applies_clip_particles_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(
            target[8 * 16 + 4],
            premul_f32_to_u32(red.premultiply().components)
        );
        assert_eq!(target[8 * 16 + 12], 0);
    }

    #[test]
    fn fine_wgpu_applies_opacity_layer_stack_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_opacity_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[8 * 16 + 4], rgba8_pack([128, 0, 0, 128]));
        assert_eq!(target[8 * 16 + 12], 0);
    }

    #[test]
    fn fine_wgpu_applies_blend_layer_stack_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let blue = Color::from_rgb8(0, 0, 255);
        let mut scene = Scene::new(16, 16);
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), blue, FillRule::NonZero);
        scene.push_blend_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 0, 0, 255]));
        assert_eq!(
            target[8 * 16 + 12],
            premul_f32_to_u32(blue.premultiply().components)
        );
    }

    #[test]
    fn filter_wgpu_applies_color_filter_to_offscreen_children_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_filter_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 255, 255, 255]));
        assert_eq!(target[8 * 16 + 12], 0);
    }

    #[test]
    fn filter_wgpu_blur_outputs_expanded_bounds_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(96, 96);
        let sample_rect = Rect::new(32.0, 32.0, 64.0, 64.0);
        scene.push_filter_layer(
            Filter::Blur(4.0),
            Region::rect(sample_rect, Radius::all(0.0)),
        );
        scene.push_rect(sample_rect, Color::from_rgb8(255, 0, 0), FillRule::NonZero);
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(96, 96, Color::WHITE);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());
        let expanded_px = unpack_rgba8(target[48 * 96 + 28]);
        let far_px = unpack_rgba8(target[48 * 96 + 16]);

        assert_eq!(expanded_px[0], 255);
        assert!(
            expanded_px[1] < 245 && expanded_px[2] < 245,
            "expected blur outside sample region, got {expanded_px:?}"
        );
        assert_eq!(far_px, [255, 255, 255, 255]);
    }

    #[test]
    fn filter_wgpu_drop_shadow_offsets_alpha_and_preserves_source_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(8, 8);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 2.0,
                offset_y: 1.0,
                radius: 0.0,
                brush: Brush::Solid(Color::BLACK),
            },
            Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(2.0, 2.0, 3.0, 3.0),
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[2 * 8 + 2], rgba8_pack([255, 255, 255, 255]));
        assert_eq!(target[3 * 8 + 4], rgba8_pack([0, 0, 0, 255]));
        assert_eq!(target[1 * 8 + 1], 0);
    }

    #[test]
    fn filter_wgpu_drop_shadow_blurs_offset_alpha_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(32, 32);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 0.0,
                offset_y: 8.0,
                radius: 2.0,
                brush: Brush::Solid(Color::BLACK),
            },
            Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(8.0, 8.0, 16.0, 16.0),
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(32, 32, Color::TRANSPARENT);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());
        let shadow_px = unpack_rgba8(target[25 * 32 + 12]);
        let source_px = unpack_rgba8(target[12 * 32 + 12]);
        let far_px = unpack_rgba8(target[31 * 32 + 12]);

        assert_eq!(source_px, [255, 255, 255, 255]);
        assert_eq!(shadow_px[0..3], [0, 0, 0]);
        assert!(
            shadow_px[3] > 0 && shadow_px[3] < 255,
            "expected blurred shadow edge, got {shadow_px:?}"
        );
        assert_eq!(far_px, [0, 0, 0, 0]);
    }

    #[test]
    fn filter_wgpu_drop_shadow_samples_linear_gradient_brush_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let shadow = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
            .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
        let mut scene = Scene::new(32, 48);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 0.0,
                offset_y: 16.0,
                radius: 0.0,
                brush: Brush::from_gradient(&shadow),
            },
            Region::rect(Rect::new(0.0, 0.0, 32.0, 48.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(32, 48, Color::TRANSPARENT);
        renderer.render(&scene);
        assert_eq!(renderer.filter_brushes.data.read(renderer.client())[0], 2);
        let payload = renderer.filter_brushes.payloads.read(renderer.client());
        assert_eq!(unpack_rgba8(payload[0]), [255, 0, 0, 255]);
        assert_eq!(
            *payload.last().map(|px| unpack_rgba8(*px)).as_ref().unwrap(),
            [0, 0, 255, 255]
        );
        let target = renderer.target.read(renderer.client());
        let left_shadow = unpack_rgba8(target[20 * 32 + 4]);
        let right_shadow = unpack_rgba8(target[20 * 32 + 27]);

        assert_eq!(left_shadow[3], 255);
        assert_eq!(right_shadow[3], 255);
        assert!(
            left_shadow[0] > left_shadow[2],
            "expected red side of gradient shadow, got {left_shadow:?}"
        );
        assert!(
            right_shadow[2] > right_shadow[0],
            "expected blue side of gradient shadow, got {right_shadow:?}"
        );
    }

    #[test]
    fn filter_wgpu_drop_shadow_samples_pattern_brush_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let pattern = Brush::Pattern(PatternBrush {
            image: Arc::new(Image {
                width: 2,
                height: 1,
                pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])],
            }),
            transform: IDENTITY_TRANSFORM,
            opacity: 255,
        });
        let mut scene = Scene::new(16, 48);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 0.0,
                offset_y: 16.0,
                radius: 0.0,
                brush: pattern,
            },
            Region::rect(Rect::new(0.0, 0.0, 16.0, 48.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 48, Color::TRANSPARENT);
        renderer.render(&scene);
        assert_eq!(renderer.filter_brushes.data.read(renderer.client())[0], 6);
        assert_eq!(
            renderer.filter_brushes.payloads.read(renderer.client()),
            vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])]
        );
        let target = renderer.target.read(renderer.client());

        assert_eq!(unpack_rgba8(target[20 * 16]), [255, 0, 0, 255]);
        assert_eq!(unpack_rgba8(target[20 * 16 + 1]), [0, 0, 255, 255]);
        assert_eq!(unpack_rgba8(target[20 * 16 + 2]), [255, 0, 0, 255]);
    }

    #[test]
    fn filter_wgpu_drop_shadow_samples_radial_gradient_brush_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let shadow = Gradient::new_radial((16.0, 24.0), 10.0)
            .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
        let mut scene = Scene::new(32, 48);
        scene.push_filter_layer(
            Filter::DropShadow {
                offset_x: 0.0,
                offset_y: 16.0,
                radius: 0.0,
                brush: Brush::from_gradient(&shadow),
            },
            Region::rect(Rect::new(0.0, 0.0, 32.0, 48.0), Radius::all(0.0)),
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(32, 48, Color::TRANSPARENT);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());
        let center = unpack_rgba8(target[24 * 32 + 16]);
        let edge = unpack_rgba8(target[24 * 32 + 26]);

        assert!(
            center[0] > center[2],
            "expected red radial center, got {center:?}"
        );
        assert!(edge[2] > edge[0], "expected blue radial edge, got {edge:?}");
    }

    #[test]
    fn backdrop_wgpu_applies_color_filter_to_existing_target_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );
        scene.push_backdrop_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(4.0, 0.0, 12.0, 16.0), Radius::all(0.0)),
        );
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[8 * 16 + 8], rgba8_pack([0, 255, 255, 255]));
        assert_eq!(target[8 * 16 + 2], rgba8_pack([255, 0, 0, 255]));
        assert_eq!(target[8 * 16 + 14], rgba8_pack([255, 0, 0, 255]));
    }

    #[test]
    fn fine_wgpu_does_not_leak_clip_after_layer_pop_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let blue = Color::from_rgb8(0, 0, 255);
        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), blue, FillRule::NonZero);

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(
            target[8 * 16 + 4],
            premul_f32_to_u32(blue.premultiply().components)
        );
        assert_eq!(
            target[8 * 16 + 12],
            premul_f32_to_u32(blue.premultiply().components)
        );
    }

    #[test]
    fn fine_wgpu_intersects_nested_clip_layers_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let red = Color::from_rgb8(255, 0, 0);
        let mut scene = Scene::new(16, 16);
        scene.push_clip_layer(
            Rect::new(0.0, 0.0, 12.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_clip_layer(
            Rect::new(4.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
        scene.pop_layer();
        scene.pop_layer();

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());
        let red_px = premul_f32_to_u32(red.premultiply().components);

        assert_eq!(target[8 * 16 + 2], 0);
        assert_eq!(target[8 * 16 + 8], red_px);
        assert_eq!(target[8 * 16 + 14], 0);
    }

    #[test]
    fn fine_wgpu_uses_premultiplied_clear_color_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let clear = Color::from_rgba8(255, 0, 0, 128);
        let scene = Scene::new(4, 4);
        let mut renderer = WgpuRenderer::new_default_device(4, 4, clear);
        renderer.render_flat(&scene);
        let target = renderer.target.read(renderer.client());

        assert_eq!(target[0], premul_f32_to_u32(clear.premultiply().components));
    }
}
