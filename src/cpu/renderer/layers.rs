use peniko::{BlendMode, Color};

use crate::{
    TextFontSystem,
    cpu::{
        buffers::RasterBuffers,
        computes::blend::{composite_blend_masked_at, composite_src_over_masked_at},
        mask::{
            apply_opacity_to_mask, copy_image_region, intersect_alpha_mask,
            intersect_sdf_alpha_mask, rasterize_layer_mask, rasterize_region_mask, region_bounds,
            svg_mask_coverage,
        },
        offscreen::OffscreenSurface,
        pipelines::runner::{run_cumsum, run_scan},
    },
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        image::Image,
        layer::{Layer, filter::Filter, mask::Mask, region::Region},
    },
    text::{PreparedTextData, TextContext},
};

use super::Renderer;

pub(super) struct OffscreenLayerRef<'a> {
    pub(super) draw: usize,
    pub(super) layer: &'a Layer,
    pub(super) outer_stack: std::ops::Range<usize>,
    pub(super) children: &'a [ExecOp],
}

pub(super) struct MaskLayerRef<'a> {
    pub(super) layer: &'a Mask,
    pub(super) outer_stack: std::ops::Range<usize>,
    pub(super) content: &'a [ExecOp],
    pub(super) mask: &'a [ExecOp],
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

impl Renderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_offscreen_layer(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        offscreen: OffscreenLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        match offscreen.layer {
            Layer::Isolate => self.execute_masked_group_layer(
                canvas,
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
                text_data,
                text_context,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                canvas,
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
                text_data,
                text_context,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                canvas,
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
                text_data,
                text_context,
            ),
            Layer::ClipSdf { .. } => unreachable!("ClipSdf layers are fused into analytic clips"),
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer(
                canvas,
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
                text_data,
                text_context,
            ),
            Layer::Backdrop {
                filter,
                sample_region,
            } => self.execute_backdrop_layer(
                canvas,
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
                text_data,
                text_context,
            ),
            _ => unreachable!(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_filter_layer(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        layer: FilterLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        _text_data: Option<&PreparedTextData>,
        text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        let Some(filter_bounds) =
            self.filter
                .surface_bounds(layer.filter, layer.sample_region, target_bounds)
        else {
            return;
        };

        let mut surface =
            OffscreenSurface::new(canvas, plan, layer.children, filter_bounds.surface);
        self.render_offscreen_surface(&mut surface, text_context);
        self.filter
            .prepare(
                &mut surface.image,
                layer.filter,
                filter_bounds.surface,
                Some(&self.image_resources),
            )
            .run();

        let output = copy_image_region(&surface.image, filter_bounds.output, filter_bounds.surface);
        let mut mask = Image::new(
            filter_bounds.output.width(),
            filter_bounds.output.height(),
            Color::WHITE,
        );
        self.apply_outer_clip_stack_to_mask(
            canvas,
            plan,
            layer.outer_stack,
            filter_bounds.output,
            &mut mask,
            buffers,
        );
        composite_src_over_masked_at(target, &output, &mask, filter_bounds.output, target_bounds);
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_backdrop_layer(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        layer: BackdropLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        let bounds = self.filter.filtered_region_bounds(
            layer.filter,
            layer.sample_region,
            Bounds::canvas(canvas.width, canvas.height),
        );
        if bounds.is_empty() {
            return;
        }

        let mut backdrop = copy_image_region(target, bounds, target_bounds);
        self.filter
            .prepare_backdrop(
                &mut backdrop,
                layer.filter,
                bounds,
                (target_bounds.width(), target_bounds.height()),
                layer.sample_region,
                Some(&self.image_resources),
            )
            .run();

        let mut backdrop_mask = rasterize_region_mask(layer.sample_region, bounds);
        self.apply_outer_clip_stack_to_mask(
            canvas,
            plan,
            layer.outer_stack.clone(),
            bounds,
            &mut backdrop_mask,
            buffers,
        );
        composite_src_over_masked_at(target, &backdrop, &backdrop_mask, bounds, target_bounds);

        let content = self.render_children_to_image(
            canvas,
            plan,
            layer.children,
            target_bounds,
            buffers,
            text_data,
            text_context,
        );
        let mut content_mask =
            Image::new(target_bounds.width(), target_bounds.height(), Color::WHITE);
        self.apply_outer_clip_stack_to_mask(
            canvas,
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

    #[allow(clippy::too_many_arguments)]
    fn execute_masked_group_layer(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        group: MaskedGroupLayer<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        let bounds = draw_bounds(canvas, group.draw).intersect(target_bounds);
        if bounds.is_empty() {
            return;
        }

        let image = self.render_children_to_image(
            canvas,
            plan,
            group.children,
            bounds,
            buffers,
            text_data,
            text_context,
        );
        let mut mask = rasterize_layer_mask(canvas, group.draw, bounds, buffers);
        if let Some(opacity) = group.opacity {
            apply_opacity_to_mask(&mut mask, opacity);
        }
        self.apply_outer_clip_stack_to_mask(
            canvas,
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

    #[allow(clippy::too_many_arguments)]
    fn render_children_to_image(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        children: &[ExecOp],
        bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) -> Image {
        let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
        self.execute_ops(
            canvas,
            plan,
            children,
            &mut image,
            bounds,
            buffers,
            text_data,
            text_context,
        );
        image
    }

    fn render_offscreen_surface(
        &mut self,
        surface: &mut OffscreenSurface,
        mut text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        run_scan(&self.scan, &surface.canvas, &mut surface.buffers);
        run_cumsum(&self.cumsum, &surface.canvas, &mut surface.buffers);
        let text_data = text_context.as_mut().map(|(font_system, context)| {
            PreparedTextData::new(
                &surface.canvas.text_glyphs,
                &surface.canvas.text_runs,
                font_system,
                context,
            )
        });
        self.execute_ops(
            &surface.canvas,
            &surface.plan,
            &surface.children,
            &mut surface.image,
            Bounds::canvas(surface.bounds.width(), surface.bounds.height()),
            &mut surface.buffers,
            text_data.as_ref(),
            text_context,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_mask_layer(
        &mut self,
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        mask_layer: MaskLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
        buffers: &mut RasterBuffers,
        text_data: Option<&PreparedTextData>,
        mut text_context: Option<(&mut TextFontSystem, &mut TextContext)>,
    ) {
        let bounds = region_bounds(&mask_layer.layer.region).intersect(target_bounds);
        if bounds.is_empty() {
            return;
        }

        let content = self.render_children_to_image(
            canvas,
            plan,
            mask_layer.content,
            bounds,
            buffers,
            text_data,
            text_context
                .as_mut()
                .map(|(font_system, context)| (&mut **font_system, &mut **context)),
        );
        let mask_source = self.render_children_to_image(
            canvas,
            plan,
            mask_layer.mask,
            bounds,
            buffers,
            text_data,
            text_context,
        );
        let mut mask = svg_mask_coverage(&mask_source, mask_layer.layer.kind);
        let region_mask = rasterize_region_mask(&mask_layer.layer.region, bounds);
        intersect_alpha_mask(&mut mask, &region_mask);
        self.apply_outer_clip_stack_to_mask(
            canvas,
            plan,
            mask_layer.outer_stack,
            bounds,
            &mut mask,
            buffers,
        );
        composite_src_over_masked_at(target, &content, &mask, bounds, target_bounds);
    }

    fn apply_outer_clip_stack_to_mask(
        &self,
        canvas: &crate::canvas::Canvas,
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
            let draw_ix = draw as usize;
            let draw = &canvas.draw_records[draw_ix];
            if let Some(sdf) = &draw.sdf {
                // Offscreen results already need a destination mask; apply the
                // SDF analytically to that mask instead of allocating a clip mask.
                let sdf_bounds = Bounds::new(
                    draw.pixel_bounds.x0,
                    draw.pixel_bounds.y0,
                    draw.pixel_bounds.x1,
                    draw.pixel_bounds.y1,
                );
                intersect_sdf_alpha_mask(mask, sdf, sdf_bounds, bounds);
            } else {
                let clip = rasterize_layer_mask(canvas, draw_ix, bounds, buffers);
                intersect_alpha_mask(mask, &clip);
            }
        }
    }
}

fn draw_bounds(canvas: &crate::canvas::Canvas, draw_ix: usize) -> Bounds {
    let bounds = canvas.draw_records[draw_ix].pixel_bounds;
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
