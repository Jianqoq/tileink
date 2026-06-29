use std::time::Duration;

use peniko::{BlendMode, Color};

use crate::{
    cpu::{
        buffers::RasterBuffers,
        computes::blend::{composite_blend_masked_at, composite_src_over_masked_at},
        mask::{
            apply_opacity_to_mask, copy_image_region, intersect_alpha_mask, rasterize_layer_mask,
            rasterize_region_mask, rasterize_sdf_mask, region_bounds, svg_mask_coverage,
        },
        offscreen::OffscreenSurface,
        pipelines::{
            coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, filter::FilterCpuPipeline,
            fine::FineCpuPipeline, scan::ScanCpuPipeline,
        },
    },
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    shared::{
        bounds::Bounds,
        draw_record::DrawRecord,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        image::Image,
        layer::{Layer, filter::Filter, mask::Mask, region::Region},
    },
};

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

struct OffscreenLayerRef<'a> {
    draw: usize,
    layer: &'a Layer,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

struct MaskLayerRef<'a> {
    layer: &'a Mask,
    outer_stack: std::ops::Range<usize>,
    content: &'a [ExecOp],
    mask: &'a [ExecOp],
}

struct FilterLayerRef<'a> {
    filter: &'a Filter,
    sample_region: &'a Region,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

struct BackdropLayerRef<'a> {
    filter: &'a Filter,
    sample_region: &'a Region,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

struct MaskedGroupLayer<'a> {
    draw: usize,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
    opacity: Option<f32>,
    composite: LayerComposite,
}

#[derive(Clone, Copy)]
enum LayerComposite {
    SrcOver,
    Blend(BlendMode),
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
        run_scan_pipeline(&self.scan, scene, &mut self.main);
    }

    fn cumsum(&mut self, scene: &crate::scene::Scene, _: Self::CumsumArgs<'_>) {
        run_cumsum_pipeline(&self.cumsum, scene, &mut self.main);
    }

    fn coarse(&mut self, scene: &crate::scene::Scene, args: Self::CoarseArgs<'_>) {
        let (draw_records, draw_range, layer_stack_data, layer_stack_range) = args;
        run_coarse_pipeline(
            &self.coarse,
            scene,
            draw_records,
            draw_range,
            layer_stack_data,
            layer_stack_range,
            &mut self.main,
        );
    }

    fn fine(&mut self, scene: &crate::scene::Scene, target: Self::FineArgs<'_>) {
        let (target, target_bounds) = target;
        run_fine_pipeline(&self.fine, scene, target, target_bounds, &self.main);
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

    fn execute_offscreen_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        offscreen: OffscreenLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        match offscreen.layer {
            Layer::Isolate => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: None,
                    composite: LayerComposite::SrcOver,
                },
                target,
                target_bounds,
                buffers,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: Some(opacity.opacity),
                    composite: LayerComposite::SrcOver,
                },
                target,
                target_bounds,
                buffers,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: None,
                    composite: LayerComposite::Blend(blend.mode),
                },
                target,
                target_bounds,
                buffers,
            ),
            Layer::ClipSdf { sdf, bounds } => {
                if bounds.intersect(target_bounds).is_empty() {
                    return;
                }
                let image = self.render_children_to_image(
                    scene,
                    plan,
                    offscreen.children,
                    target_bounds,
                    buffers,
                );

                let mut mask = rasterize_sdf_mask(sdf, *bounds, target_bounds);
                self.apply_outer_clip_stack_to_mask(
                    scene,
                    plan,
                    offscreen.outer_stack,
                    target_bounds,
                    &mut mask,
                    buffers,
                );
                composite_src_over_masked_at(target, &image, &mask, target_bounds, target_bounds);
            }
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer(
                scene,
                plan,
                FilterLayerRef {
                    filter,
                    sample_region,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                },
                target,
                target_bounds,
                buffers,
            ),
            Layer::Backdrop {
                filter,
                sample_region,
            } => self.execute_backdrop_layer(
                scene,
                plan,
                BackdropLayerRef {
                    filter,
                    sample_region,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                },
                target,
                target_bounds,
                buffers,
            ),
            _ => unreachable!(),
        }
    }

    fn execute_filter_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        layer: FilterLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        let Some(filter_bounds) =
            self.filter
                .surface_bounds(layer.filter, layer.sample_region, target_bounds)
        else {
            return;
        };

        let mut surface = OffscreenSurface::new(scene, plan, layer.children, filter_bounds.surface);
        self.render_offscreen_surface(&mut surface);
        self.filter
            .prepare(&mut surface.image, layer.filter, filter_bounds.surface)
            .run();

        let output = copy_image_region(&surface.image, filter_bounds.output, filter_bounds.surface);
        let mut mask = Image::new(
            filter_bounds.output.width(),
            filter_bounds.output.height(),
            Color::WHITE,
        );
        self.apply_outer_clip_stack_to_mask(
            scene,
            plan,
            layer.outer_stack,
            filter_bounds.output,
            &mut mask,
            buffers,
        );
        composite_src_over_masked_at(target, &output, &mask, filter_bounds.output, target_bounds);
    }

    fn execute_backdrop_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        layer: BackdropLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        let bounds = self.filter.filtered_region_bounds(
            layer.filter,
            layer.sample_region,
            Bounds::canvas(scene.width, scene.height),
        );
        if bounds.is_empty() {
            return;
        }

        let mut backdrop = copy_image_region(target, bounds, target_bounds);
        self.filter
            .prepare(&mut backdrop, layer.filter, bounds)
            .run();

        let mut backdrop_mask = rasterize_region_mask(layer.sample_region, bounds);
        self.apply_outer_clip_stack_to_mask(
            scene,
            plan,
            layer.outer_stack.clone(),
            bounds,
            &mut backdrop_mask,
            buffers,
        );
        composite_src_over_masked_at(target, &backdrop, &backdrop_mask, bounds, target_bounds);

        let content =
            self.render_children_to_image(scene, plan, layer.children, target_bounds, buffers);
        let mut content_mask =
            Image::new(target_bounds.width(), target_bounds.height(), Color::WHITE);
        self.apply_outer_clip_stack_to_mask(
            scene,
            plan,
            layer.outer_stack,
            target_bounds,
            &mut content_mask,
            buffers,
        );
        composite_src_over_masked_at(
            target,
            &content,
            &content_mask,
            target_bounds,
            target_bounds,
        );
    }

    fn execute_masked_group_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        group: MaskedGroupLayer<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        let bounds = draw_bounds(scene, group.draw).intersect(target_bounds);
        if bounds.is_empty() {
            return;
        }

        let image = self.render_children_to_image(scene, plan, group.children, bounds, buffers);
        let mut mask = rasterize_layer_mask(scene, group.draw, bounds, buffers);
        if let Some(opacity) = group.opacity {
            apply_opacity_to_mask(&mut mask, opacity);
        }
        self.apply_outer_clip_stack_to_mask(
            scene,
            plan,
            group.outer_stack,
            bounds,
            &mut mask,
            buffers,
        );
        composite_masked_layer(
            target,
            &image,
            &mask,
            bounds,
            target_bounds,
            group.composite,
        );
    }

    fn render_children_to_image(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        children: &[ExecOp],
        bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) -> Image {
        let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
        self.execute_ops(scene, plan, children, &mut image, bounds, buffers);
        image
    }

    fn render_offscreen_surface(&mut self, surface: &mut OffscreenSurface) {
        run_scan_pipeline(&self.scan, &surface.scene, &mut surface.buffers);
        run_cumsum_pipeline(&self.cumsum, &surface.scene, &mut surface.buffers);
        self.execute_ops(
            &surface.scene,
            &surface.plan,
            &surface.children,
            &mut surface.image,
            Bounds::canvas(surface.bounds.width(), surface.bounds.height()),
            &mut surface.buffers,
        );
    }

    fn execute_mask_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        mask_layer: MaskLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
    ) {
        let bounds = region_bounds(&mask_layer.layer.region).intersect(target_bounds);
        if bounds.is_empty() {
            return;
        }

        let content =
            self.render_children_to_image(scene, plan, mask_layer.content, bounds, buffers);
        let mask_source =
            self.render_children_to_image(scene, plan, mask_layer.mask, bounds, buffers);
        let mut mask = svg_mask_coverage(&mask_source, mask_layer.layer.kind);
        let region_mask = rasterize_region_mask(&mask_layer.layer.region, bounds);
        intersect_alpha_mask(&mut mask, &region_mask);
        self.apply_outer_clip_stack_to_mask(
            scene,
            plan,
            mask_layer.outer_stack,
            bounds,
            &mut mask,
            buffers,
        );
        composite_src_over_masked_at(target, &content, &mask, bounds, target_bounds);
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
        run_coarse_pipeline(
            &self.coarse,
            scene,
            &scene.draw_records,
            start..end,
            &plan.layer_stack_data,
            layer_stack,
            buffers,
        );
        run_fine_pipeline(&self.fine, scene, target, target_bounds, buffers);
    }

    fn apply_outer_clip_stack_to_mask(
        &self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        outer_stack: std::ops::Range<usize>,
        bounds: Bounds,
        mask: &mut Image,
        buffers: &RasterBuffers,
    ) {
        for entry in &plan.layer_stack_data[outer_stack] {
            let LayerStackEntry::Clip { draw } = *entry else {
                continue;
            };
            let clip = rasterize_layer_mask(scene, draw as usize, bounds, buffers);
            intersect_alpha_mask(mask, &clip);
        }
    }
}

fn draw_bounds(scene: &crate::scene::Scene, draw_ix: usize) -> Bounds {
    let bounds = scene.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

fn composite_masked_layer(
    target: &mut Image,
    content: &Image,
    mask: &Image,
    content_bounds: Bounds,
    target_bounds: Bounds,
    composite: LayerComposite,
) {
    match composite {
        LayerComposite::SrcOver => {
            composite_src_over_masked_at(target, content, mask, content_bounds, target_bounds)
        }
        LayerComposite::Blend(mode) => {
            composite_blend_masked_at(target, content, mask, content_bounds, target_bounds, mode)
        }
    }
}

fn run_scan_pipeline(
    scan: &ScanCpuPipeline,
    scene: &crate::scene::Scene,
    buffers: &mut RasterBuffers,
) {
    let Some(last_bd_record) = scene.bd_records.last().copied() else {
        buffers.clear_scan_outputs();
        return;
    };
    buffers.resize_scan_outputs(scene, last_bd_record);
    scan.prepare(
        &scene.lines,
        &scene.path_records,
        &scene.bd_records,
        &mut buffers.backdrops,
        &mut buffers.tile_segment_ranges,
        &mut buffers.segments,
        &mut buffers.segments_bump,
        &mut buffers.segment_tile_counts,
        &mut buffers.segment_tile_cursors,
        (scene.width_in_tiles(), scene.height_in_tiles()),
    )
    .run();
}

fn run_cumsum_pipeline(
    cumsum: &CumsumCpuPipeline,
    scene: &crate::scene::Scene,
    buffers: &mut RasterBuffers,
) {
    cumsum
        .prepare(&mut buffers.backdrops, &scene.bd_records)
        .run();
}

#[allow(clippy::too_many_arguments)]
fn run_coarse_pipeline(
    coarse: &CoarseCpuPipeline,
    scene: &crate::scene::Scene,
    draw_records: &[DrawRecord],
    draw_range: std::ops::Range<usize>,
    layer_stack_data: &[LayerStackEntry],
    layer_stack_range: std::ops::Range<usize>,
    buffers: &mut RasterBuffers,
) {
    coarse
        .prepare(
            draw_records,
            draw_range,
            layer_stack_data,
            layer_stack_range,
            &scene.bd_records,
            &buffers.backdrops,
            &buffers.tile_segment_ranges,
            &mut buffers.tile_ptcl_ranges,
            &mut buffers.tile_ptcls,
            (scene.width_in_tiles(), scene.height_in_tiles()),
        )
        .run();
}

fn run_fine_pipeline(
    fine: &FineCpuPipeline,
    scene: &crate::scene::Scene,
    target: &mut Image,
    target_bounds: Bounds,
    buffers: &RasterBuffers,
) {
    fine.prepare(
        &buffers.tile_ptcl_ranges,
        &buffers.tile_ptcls,
        &buffers.segments,
        target,
        target_bounds,
        (scene.width_in_tiles(), scene.height_in_tiles()),
    )
    .run();
}

#[cfg(test)]
mod tests;
