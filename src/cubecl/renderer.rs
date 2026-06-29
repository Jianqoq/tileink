use ::cubecl::prelude::Runtime;
use peniko::Color;

mod executor;
mod resources;
mod scratch;
mod target;
use executor::{
    FilterConvolveBuffers, FilterConvolveUpload, FilterPathBuffers, FilterPathUpload,
    FilterTransferBuffers, FilterTransferUpload, FilterTurbulenceBuffers, FilterTurbulenceUpload,
};
use resources::SceneUploadStaging;
pub(crate) use resources::{CoarseBuffers, ScanBuffers, SceneBuffers};
use scratch::{plan_stack_depths, required_scratch_count};
use target::CubeRenderTarget;

use crate::{
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    scene::Scene,
    shared::{
        execution::{ExecPlan, ROOT_COMMAND_LIST_ID},
        image::{Image, premul_color_to_rgba8_pack},
        line_seg::LineSegment,
        tile_seg_range::TileSegmentRange,
    },
};

use super::{
    brush::{GpuBrushBuffers, GpuBrushUpload},
    buffer::CubeBuffer,
    pipelines::{
        coarse::{CoarseBatch, CoarsePipeline},
        cumsum::CumsumPipeline,
        fine::{FinePipeline, FineRenderConfig},
        scan::ScanPipeline,
    },
    types::{CubeBufferLengths, CubeSceneConfig},
};

pub type WgpuRenderer = Renderer<::cubecl::wgpu::WgpuRuntime>;

/// CubeCL renderer resource owner.
///
/// This replaces the direct wgpu backend. Kernels will be written with
/// `#[cube]` and launched through CubeCL; with the current Cargo features those
/// kernels compile to the CubeCL wgpu runtime.
pub struct Renderer<R: Runtime> {
    client: ::cubecl::client::ComputeClient<R>,
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
    scene_upload: SceneUploadStaging,
    draw_brushes: GpuBrushBuffers,
    filter_brushes: GpuBrushBuffers,
    filter_convolves: FilterConvolveBuffers,
    filter_paths: FilterPathBuffers,
    filter_transfers: FilterTransferBuffers,
    filter_turbulence: FilterTurbulenceBuffers,
    target: CubeBuffer<u32>,
    scratch: Vec<CubeBuffer<u32>>,
    scratch_in_use: Vec<bool>,
    surface_sources: Vec<CubeBuffer<u32>>,
    surface_origin: (i32, i32),
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
            scene_upload: SceneUploadStaging::default(),
            draw_brushes: GpuBrushBuffers::new(&client),
            filter_brushes: GpuBrushBuffers::new(&client),
            filter_convolves: FilterConvolveBuffers::new(&client),
            filter_paths: FilterPathBuffers::new(&client),
            filter_transfers: FilterTransferBuffers::new(&client),
            filter_turbulence: FilterTurbulenceBuffers::new(&client),
            target: CubeBuffer::new(&client, width as usize * height as usize),
            scratch: Vec::new(),
            scratch_in_use: Vec::new(),
            surface_sources: Vec::new(),
            surface_origin: (0, 0),
            client,
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
    fn prepare_scene(&mut self, scene: &Scene) {
        self.surface_sources.clear();
        self.surface_origin = (0, 0);
        self.resize(scene.width, scene.height);
        let lengths = CubeBufferLengths::from_scene(scene);
        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        let (max_clip_depth, max_group_depth) = plan_stack_depths(&plan);
        let scratch_count = required_scratch_count(&plan);
        let draw_brush_upload = GpuBrushUpload::from_scene_draws(scene);
        let filter_brush_upload = GpuBrushUpload::from_filter_plan(&plan.ops);
        let filter_convolve_upload = FilterConvolveUpload::from_plan(&plan);
        let filter_path_upload = FilterPathUpload::from_plan(&plan);
        let filter_transfer_upload = FilterTransferUpload::from_plan(&plan);
        let filter_turbulence_upload = FilterTurbulenceUpload::from_plan(&plan);
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.prepare_scratch_buffers(scratch_count);
        self.draw_brushes.upload(&self.client, draw_brush_upload);
        self.filter_brushes
            .upload(&self.client, filter_brush_upload);
        self.filter_convolves
            .upload(&self.client, filter_convolve_upload);
        self.filter_paths.upload(&self.client, filter_path_upload);
        self.filter_transfers
            .upload(&self.client, filter_transfer_upload);
        self.filter_turbulence
            .upload(&self.client, filter_turbulence_upload);
        self.scene
            .upload(&self.client, scene, &plan, &mut self.scene_upload);
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

    /// Renders a scene and returns backend-neutral debug data without writing files.
    pub fn render_with_options(
        &mut self,
        scene: &Scene,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.render(scene);
        let image = self.image();
        let (tile_segment_ranges, segments, backdrops) = self.read_debug_scan_buffers();
        capture_render_debug(
            "cubecl",
            scene,
            &image,
            DebugScanBuffers {
                backdrops: &backdrops,
                tile_segment_ranges: &tile_segment_ranges,
                segments: &segments,
            },
            options,
        )
    }

    fn read_debug_scan_buffers(&self) -> (Vec<TileSegmentRange>, Vec<LineSegment>, Vec<i32>) {
        let starts = self.scan.tile_segment_range_starts.read(&self.client);
        let ends = self.scan.tile_segment_range_ends.read(&self.client);
        let tile_segment_ranges = starts
            .into_iter()
            .zip(ends)
            .map(|(start, end)| TileSegmentRange { start, end })
            .collect();

        let p0x = self.scan.segment_p0x.read(&self.client);
        let p0y = self.scan.segment_p0y.read(&self.client);
        let p1x = self.scan.segment_p1x.read(&self.client);
        let p1y = self.scan.segment_p1y.read(&self.client);
        let y_edge = self.scan.segment_y_edge.read(&self.client);
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

        let backdrops = self.scan.backdrops.read(&self.client);
        (tile_segment_ranges, segments, backdrops)
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn prepare_scene_for_bench(&mut self, scene: &Scene) {
        self.prepare_scene(scene);
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

    fn resize(&mut self, width: u32, height: u32) {
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

    #[cfg(any(test, feature = "bench-api"))]
    fn client(&self) -> &::cubecl::client::ComputeClient<R> {
        &self.client
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn client_for_bench(&self) -> &::cubecl::client::ComputeClient<R> {
        self.client()
    }
}

impl WgpuRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
    }
}

#[cfg(test)]
mod tests;
