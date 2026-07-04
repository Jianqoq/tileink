use std::{sync::Arc as SharedArc, time::Duration};

use peniko::Color;

use crate::{
    TextFontSystem,
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
        image_resource::{ImageKey, ImageResourceStore},
    },
    text::{PreparedTextData, TextContext},
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
    image_resources: ImageResourceStore,
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
        Option<&'a PreparedTextData>,
    );

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, canvas: &crate::canvas::Canvas) {
        self.size = (canvas.width, canvas.height);
        let mut image = Image::new(canvas.width, canvas.height, self.clear);
        self.execute(canvas, &mut image);
        self.image = image;
    }

    fn execute(&mut self, canvas: &crate::canvas::Canvas, args: Self::ExecuteArgs<'_>) {
        let plan = canvas.compile(0);
        self.scan(canvas, ());
        self.cumsum(canvas, ());
        self.execute_plan(canvas, &plan, args, None);
    }

    fn scan(&mut self, canvas: &crate::canvas::Canvas, _: Self::ScanArgs<'_>) {
        run_scan(&self.scan, canvas, &mut self.main);
    }

    fn cumsum(&mut self, canvas: &crate::canvas::Canvas, _: Self::CumsumArgs<'_>) {
        run_cumsum(&self.cumsum, canvas, &mut self.main);
    }

    fn coarse(&mut self, canvas: &crate::canvas::Canvas, args: Self::CoarseArgs<'_>) {
        let (draw_records, draw_range, layer_stack_data, layer_stack_range, text) = args;
        run_coarse(
            &self.coarse,
            canvas,
            CoarseStage {
                draw_records,
                draw_range,
                layer_stack_data,
                layer_stack_range,
                text,
            },
            &mut self.main,
        );
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
            image_resources: ImageResourceStore::default(),
            size: (width, height),
            main: RasterBuffers::default(),
        }
    }

    pub fn render(&mut self, canvas: &crate::canvas::Canvas) {
        <Self as Render>::render(self, canvas);
    }

    pub fn set_clear_color(&mut self, clear: Color) {
        self.clear = clear;
    }

    pub fn insert_image(&mut self, key: ImageKey, image: impl Into<SharedArc<Image>>) -> bool {
        self.image_resources.insert(key, image)
    }

    pub fn remove_image(&mut self, key: ImageKey) -> bool {
        self.image_resources.remove(key)
    }

    pub fn clear_images(&mut self) -> bool {
        self.image_resources.clear()
    }

    pub fn image_resource(&self, key: ImageKey) -> Option<&Image> {
        self.image_resources.get(key)
    }

    /// Renders text draws using the same font system that created their
    /// [`TextLayout`](crate::TextLayout). Cosmic glyph cache keys contain
    /// font ids, so using a different font system can make those keys refer to
    /// the wrong font.
    pub fn render_with_text(
        &mut self,
        canvas: &crate::canvas::Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        self.size = (canvas.width, canvas.height);
        let mut image = Image::new(canvas.width, canvas.height, self.clear);
        self.execute_with_text(canvas, &mut image, font_system, text_context);
        self.image = image;
    }

    /// Renders a canvas and returns backend-neutral debug data without writing files.
    pub fn render_with_options(
        &mut self,
        canvas: &crate::canvas::Canvas,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.render(canvas);
        capture_render_debug(
            "cpu",
            canvas,
            &self.image,
            DebugScanBuffers {
                backdrops: &self.main.backdrops,
                tile_segment_ranges: &self.main.tile_segment_ranges,
                segments: &self.main.segments,
            },
            options,
        )
    }

    pub fn render_profiled_flat(&mut self, canvas: &crate::canvas::Canvas) -> RenderProfile {
        let total_start = std::time::Instant::now();
        let plan = canvas.compile(0);
        let mut profile = RenderProfile::default();

        let start = std::time::Instant::now();
        self.scan(canvas, ());
        profile.scan = start.elapsed();

        let start = std::time::Instant::now();
        self.cumsum(canvas, ());
        profile.cumsum = start.elapsed();

        let mut image = Image::new(self.size.0, self.size.1, self.clear);
        let target_bounds = Bounds::canvas(canvas.width, canvas.height);

        for op in &plan.ops {
            let ExecOp::DrawBatch { draws, layer_stack } = op else {
                panic!("render_profiled_flat only supports scenes without layers");
            };

            let start = std::time::Instant::now();
            self.coarse(
                canvas,
                (
                    &canvas.draw_records,
                    draws.start..draws.end,
                    &plan.layer_stack_data,
                    layer_stack.clone(),
                    None,
                ),
            );
            profile.coarse += start.elapsed();

            let start = std::time::Instant::now();
            run_fine(
                &self.fine,
                canvas,
                &mut image,
                target_bounds,
                &self.main,
                None,
                Some(&self.image_resources),
            );
            profile.fine += start.elapsed();
        }

        self.image = image;
        profile.total = total_start.elapsed();
        profile
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    fn execute_with_text(
        &mut self,
        canvas: &crate::canvas::Canvas,
        target: &mut Image,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        let plan = canvas.compile(0);
        self.scan(canvas, ());
        self.cumsum(canvas, ());
        self.execute_plan(canvas, &plan, target, Some((font_system, text_context)));
    }

    fn execute_plan(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        target: &mut Image,
        mut text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        let mut main = std::mem::take(&mut self.main);
        let text_data = text_context.as_mut().map(|(font_system, context)| {
            PreparedTextData::new(&canvas.text_glyphs, &canvas.text_runs, font_system, context)
        });
        self.execute_ops(
            canvas,
            plan,
            &plan.ops,
            target,
            Bounds::canvas(canvas.width, canvas.height),
            &mut main,
            text_data.as_ref(),
            text_context,
        );
        self.main = main;
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_ops(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: &mut Image,
        root_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        mut text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => self.execute_draw_batch(
                    canvas,
                    plan,
                    draws.start,
                    draws.end,
                    layer_stack.clone(),
                    target,
                    root_bounds,
                    buffers,
                    text_data,
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
                    canvas,
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
                    text_data,
                    text_context
                        .as_mut()
                        .map(|(font_system, context)| (&mut **font_system, &mut **context)),
                ),
                ExecOp::OffscreenMaskLayer {
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => self.execute_mask_layer(
                    canvas,
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
                    text_data,
                    text_context
                        .as_mut()
                        .map(|(font_system, context)| (&mut **font_system, &mut **context)),
                ),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_draw_batch(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        start: usize,
        end: usize,
        layer_stack: std::ops::Range<usize>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
    ) {
        if start >= end {
            return;
        }
        run_coarse(
            &self.coarse,
            canvas,
            CoarseStage {
                draw_records: &canvas.draw_records,
                draw_range: start..end,
                layer_stack_data: &plan.layer_stack_data,
                layer_stack_range: layer_stack,
                text: text_data,
            },
            buffers,
        );
        run_fine(
            &self.fine,
            canvas,
            target,
            target_bounds,
            buffers,
            text_data,
            Some(&self.image_resources),
        );
    }
}

#[cfg(test)]
mod tests;
