use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, Color};

use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry, ROOT_COMMAND_LIST_ID},
        fill::FillRule,
        image::premul_color_to_rgba8_pack,
        line::Line,
        pixel::{opacity_f32_to_u8, premul_f32_to_u32},
    },
};

use super::{
    buffer::CubeBuffer,
    pipelines::{
        coarse::{CoarseBatch, CoarsePipeline},
        cumsum::CumsumPipeline,
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
    target: CubeBuffer<u32>,
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
            target: CubeBuffer::new(&client, width as usize * height as usize),
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
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
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

    fn fine_batch(&mut self) {
        let config = self.fine_config();
        FinePipeline::render(
            &self.client,
            &self.scan,
            &self.coarse,
            &mut self.target,
            config,
        );
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
        self.execute_ops(plan, &plan.ops);
    }

    fn execute_ops(&mut self, plan: &ExecPlan, ops: &[ExecOp]) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => {
                    self.execute_draw_batch(plan, draws.clone(), layer_stack.clone());
                }
                ExecOp::BeginClip { .. }
                | ExecOp::EndClip
                | ExecOp::BeginOpacity { .. }
                | ExecOp::EndOpacity { .. }
                | ExecOp::BeginBlend { .. }
                | ExecOp::EndBlend { .. } => {}
                ExecOp::OffscreenLayer { .. } => {
                    Self::unsupported_execute_op(op);
                }
            }
        }
    }

    fn execute_draw_batch(
        &mut self,
        _plan: &ExecPlan,
        draws: std::ops::Range<usize>,
        layer_stack: std::ops::Range<usize>,
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
        self.fine_batch();
    }

    fn unsupported_execute_op(op: &ExecOp) -> ! {
        panic!(
            "CubeCL execute currently supports flat draws and fused clip/opacity/blend layers; unsupported op: {op:?}"
        );
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
    use peniko::{
        Color, Compose, Mix,
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
    use crate::shared::execution::ExecOp;
    use crate::shared::image::rgba8_pack;
    use crate::shared::pixel::premul_f32_to_u32;
    use crate::{FillRule, Scene};

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
