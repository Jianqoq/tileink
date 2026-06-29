use std::time::Duration;

use peniko::Color;

use crate::{
    cpu::{
        buffers::RasterBuffers,
        pipelines::{
            coarse::CoarseCpuPipeline,
            cumsum::CumsumCpuPipeline,
            filter::FilterCpuPipeline,
            fine::FineCpuPipeline,
            runner::{CoarseStage, run_coarse, run_cumsum, run_fine, run_scan},
            scan::ScanCpuPipeline,
        },
    },
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    shared::{
        bounds::Bounds,
        draw_record::DrawRecord,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        image::Image,
    },
};

mod layers;
use layers::{MaskLayerRef, OffscreenLayerRef};

pub struct Renderer {
    image: Image,
    clear: Color,
    scan: ScanCpuPipeline,
    cumsum: CumsumCpuPipeline,
    coarse: CoarseCpuPipeline,
    fine: FineCpuPipeline,
    filter: FilterCpuPipeline,
    size: (u32, u32),

    main: RasterBuffers,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderProfile {
    pub scan: Duration,
    pub cumsum: Duration,
    pub coarse: Duration,
    pub fine: Duration,
    pub total: Duration,
}

impl Render for Renderer {
    type ScanArgs<'a> = ();

    type CumsumArgs<'a> = ();

    type CoarseArgs<'a> = (
        &'a [DrawRecord],
        std::ops::Range<usize>,
        &'a [LayerStackEntry],
        std::ops::Range<usize>,
    );

    type FineArgs<'a> = (&'a mut Image, Bounds);

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, scene: &crate::scene::Scene) {
        self.size = (scene.width, scene.height);
        let mut image = Image::new(scene.width, scene.height, self.clear);
        self.execute(scene, &mut image);
        self.image = image;
    }

    fn execute(&mut self, scene: &crate::scene::Scene, args: Self::ExecuteArgs<'_>) {
        let plan = scene.compile(0);
        self.scan(scene, ());
        self.cumsum(scene, ());
        self.execute_plan(scene, &plan, args);
    }

    fn scan(&mut self, scene: &crate::scene::Scene, _: Self::ScanArgs<'_>) {
        run_scan(&self.scan, scene, &mut self.main);
    }

    fn cumsum(&mut self, scene: &crate::scene::Scene, _: Self::CumsumArgs<'_>) {
        run_cumsum(&self.cumsum, scene, &mut self.main);
    }

    fn coarse(&mut self, scene: &crate::scene::Scene, args: Self::CoarseArgs<'_>) {
        let (draw_records, draw_range, layer_stack_data, layer_stack_range) = args;
        run_coarse(
            &self.coarse,
            scene,
            CoarseStage {
                draw_records,
                draw_range,
                layer_stack_data,
                layer_stack_range,
            },
            &mut self.main,
        );
    }

    fn fine(&mut self, scene: &crate::scene::Scene, target: Self::FineArgs<'_>) {
        let (target, target_bounds) = target;
        run_fine(&self.fine, scene, target, target_bounds, &self.main);
    }
}

impl Renderer {
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        Self {
            image: Image::new(width, height, clear),
            clear,
            scan: ScanCpuPipeline::new(),
            cumsum: CumsumCpuPipeline::new(),
            coarse: CoarseCpuPipeline::new(),
            fine: FineCpuPipeline::new(),
            filter: FilterCpuPipeline::new(),
            size: (width, height),
            main: RasterBuffers::default(),
        }
    }

    pub fn render(&mut self, scene: &crate::scene::Scene) {
        <Self as Render>::render(self, scene);
    }

    /// Renders a scene and returns backend-neutral debug data without writing files.
    pub fn render_with_options(
        &mut self,
        scene: &crate::scene::Scene,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.render(scene);
        capture_render_debug(
            "cpu",
            scene,
            &self.image,
            DebugScanBuffers {
                backdrops: &self.main.backdrops,
                tile_segment_ranges: &self.main.tile_segment_ranges,
                segments: &self.main.segments,
            },
            options,
        )
    }

    pub fn render_profiled_flat(&mut self, scene: &crate::scene::Scene) -> RenderProfile {
        let total_start = std::time::Instant::now();
        let plan = scene.compile(0);
        let mut profile = RenderProfile::default();

        let start = std::time::Instant::now();
        self.scan(scene, ());
        profile.scan = start.elapsed();

        let start = std::time::Instant::now();
        self.cumsum(scene, ());
        profile.cumsum = start.elapsed();

        let mut image = Image::new(self.size.0, self.size.1, self.clear);
        let target_bounds = Bounds::canvas(scene.width, scene.height);

        for op in &plan.ops {
            let ExecOp::DrawBatch { draws, layer_stack } = op else {
                panic!("render_profiled_flat only supports scenes without layers");
            };

            let start = std::time::Instant::now();
            self.coarse(
                scene,
                (
                    &scene.draw_records,
                    draws.start..draws.end,
                    &plan.layer_stack_data,
                    layer_stack.clone(),
                ),
            );
            profile.coarse += start.elapsed();

            let start = std::time::Instant::now();
            self.fine(scene, (&mut image, target_bounds));
            profile.fine += start.elapsed();
        }

        self.image = image;
        profile.total = total_start.elapsed();
        profile
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        let mut main = std::mem::take(&mut self.main);
        self.execute_ops(
            scene,
            plan,
            &plan.ops,
            target,
            Bounds::canvas(scene.width, scene.height),
            &mut main,
        );
        self.main = main;
    }

    fn execute_ops(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: &mut Image,
        root_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => self.execute_draw_batch(
                    scene,
                    plan,
                    draws.start,
                    draws.end,
                    layer_stack.clone(),
                    target,
                    root_bounds,
                    buffers,
                ),
                ExecOp::BeginClip
                | ExecOp::EndClip
                | ExecOp::BeginOpacity
                | ExecOp::EndOpacity
                | ExecOp::BeginBlend
                | ExecOp::EndBlend => {}
                ExecOp::OffscreenLayer {
                    draw,
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    scene,
                    plan,
                    OffscreenLayerRef {
                        draw: *draw,
                        layer,
                        outer_stack: outer_stack.clone(),
                        children,
                    },
                    target,
                    root_bounds,
                    buffers,
                ),
                ExecOp::OffscreenMaskLayer {
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => self.execute_mask_layer(
                    scene,
                    plan,
                    MaskLayerRef {
                        layer,
                        outer_stack: outer_stack.clone(),
                        content,
                        mask,
                    },
                    target,
                    root_bounds,
                    buffers,
                ),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        start: usize,
        end: usize,
        layer_stack: std::ops::Range<usize>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        if start >= end {
            return;
        }
        run_coarse(
            &self.coarse,
            scene,
            CoarseStage {
                draw_records: &scene.draw_records,
                draw_range: start..end,
                layer_stack_data: &plan.layer_stack_data,
                layer_stack_range: layer_stack,
            },
            buffers,
        );
        run_fine(&self.fine, scene, target, target_bounds, buffers);
    }
}

#[cfg(test)]
mod tests;
