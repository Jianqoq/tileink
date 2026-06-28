use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, Compose, Mix, kurbo::Shape};

use crate::{
    scene::Scene,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        layer::{
            Layer,
            filter::{
                ComponentTransferTable, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix,
                DiffuseLighting, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind,
                LightSource, MorphologyOperator, SpecularLighting,
            },
            mask::MaskKind,
            region::Region,
        },
        path_flatten::PathFlatten,
        pixel::opacity_f32_to_u8,
    },
};

use crate::cubecl::{
    buffer::CubeBuffer,
    pipelines::{
        filter::{
            FILTER_BRIGHTNESS, FILTER_CONTRAST, FILTER_GRAYSCALE, FILTER_HUE_ROTATE, FILTER_INVERT,
            FILTER_OPACITY, FILTER_SATURATE, FILTER_SEPIA, FilterPathResources, FilterPipeline,
            SVG_MASK_ALPHA, SVG_MASK_LUMINANCE,
        },
        fine::FinePipeline,
    },
};

use super::{CubeRenderTarget, Renderer};

#[derive(Default)]
struct FilterCursors {
    brush: usize,
    convolve: usize,
    path: usize,
    transfer: usize,
}

#[derive(Clone, Copy)]
struct MorphologyPass {
    bounds: Bounds,
    radius_x: f32,
    radius_y: f32,
    operator: MorphologyOperator,
}

#[derive(Clone, Copy)]
struct LightingDispatch {
    output_kind: u32,
    surface_scale: f32,
    light_constant: f32,
    specular_exponent: f32,
    lighting_color: [f32; 3],
    light_source: LightSource,
}

impl<R: Runtime> Renderer<R> {
    pub(super) fn execute_prepared_plan(&mut self, scene: &Scene) {
        let plan = self
            .plan
            .take()
            .expect("CubeCL execute requires prepare_scene to upload an execution plan first");
        self.execute_plan(scene, &plan);
        self.plan = Some(plan);
    }

    fn execute_plan(&mut self, scene: &Scene, plan: &ExecPlan) {
        let mut filter_cursors = FilterCursors::default();
        self.execute_ops(
            scene,
            plan,
            &plan.ops,
            CubeRenderTarget::Main,
            &mut filter_cursors,
        );
    }

    fn execute_ops(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
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
        draw: usize,
        layer: &Layer,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        match layer {
            Layer::Isolate => {
                let bounds =
                    draw_bounds(scene, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
                if bounds.is_empty() {
                    return;
                }
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(scene, plan, children, source, filter_cursors);

                let mask = self.acquire_scratch();
                self.clear_buffer(mask, 0);
                self.build_layer_mask(mask, draw as u32, bounds);
                self.composite_src_over_with_stack(target, source, Some(mask), bounds, outer_stack);
                self.release_scratch(mask);
                self.release_scratch(source);
            }
            Layer::Opacity(opacity) => {
                let bounds =
                    draw_bounds(scene, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
                if bounds.is_empty() {
                    return;
                }
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(scene, plan, children, source, filter_cursors);
                self.apply_color_filter(source, bounds, FILTER_OPACITY, opacity.opacity);

                let mask = self.acquire_scratch();
                self.clear_buffer(mask, 0);
                self.build_layer_mask(mask, draw as u32, bounds);
                self.composite_src_over_with_stack(target, source, Some(mask), bounds, outer_stack);
                self.release_scratch(mask);
                self.release_scratch(source);
            }
            Layer::Blend(blend) => {
                let bounds =
                    draw_bounds(scene, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
                if bounds.is_empty() {
                    return;
                }
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(scene, plan, children, source, filter_cursors);

                let mask = self.acquire_scratch();
                self.clear_buffer(mask, 0);
                self.build_layer_mask(mask, draw as u32, bounds);
                self.composite_blend_with_stack(
                    target,
                    source,
                    mask,
                    bounds,
                    outer_stack,
                    blend.mode,
                );
                self.release_scratch(mask);
                self.release_scratch(source);
            }
            Layer::Filter {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                next_filter_path_index(sample_region, &mut filter_cursors.path);
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.execute_ops(scene, plan, children, source, filter_cursors);
                self.apply_filter(source, bounds, filter, filter_cursors);
                self.composite_src_over_with_stack(target, source, None, bounds, outer_stack);
                self.release_scratch(source);
            }
            Layer::Backdrop {
                filter,
                sample_region,
            } => {
                let bounds = supported_filter_bounds(filter, sample_region, self.size);
                let path_index = next_filter_path_index(sample_region, &mut filter_cursors.path);
                let backdrop = self.acquire_scratch();
                self.clear_buffer(backdrop, 0);
                self.copy_region(target, backdrop, bounds);
                self.apply_filter(backdrop, bounds, filter, filter_cursors);
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
                self.execute_ops(scene, plan, children, content, filter_cursors);
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
            _ => panic!(
                "CubeCL offscreen execution only accepts Opacity, Filter, and Backdrop layers"
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_mask_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        layer: &crate::shared::layer::mask::Mask,
        outer_stack: std::ops::Range<usize>,
        content_ops: &[ExecOp],
        mask_ops: &[ExecOp],
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let bounds =
            region_bounds(&layer.region).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return;
        }
        let path_index = next_filter_path_index(&layer.region, &mut filter_cursors.path);

        let content = self.acquire_scratch();
        self.clear_buffer(content, 0);
        self.execute_ops(scene, plan, content_ops, content, filter_cursors);

        let mask_source = self.acquire_scratch();
        self.clear_buffer(mask_source, 0);
        self.execute_ops(scene, plan, mask_ops, mask_source, filter_cursors);

        let mask = self.acquire_scratch();
        self.clear_buffer(mask, 0);
        self.svg_mask_coverage(mask_source, mask, bounds, layer.kind);
        self.release_scratch(mask_source);

        let region_mask = self.acquire_scratch();
        self.clear_buffer(region_mask, 0);
        self.build_region_mask(region_mask, &layer.region, path_index, bounds);
        self.apply_region_mask(region_mask, mask, bounds);
        self.release_scratch(region_mask);

        self.composite_src_over_with_stack(target, content, Some(mask), bounds, outer_stack);
        self.release_scratch(mask);
        self.release_scratch(content);
    }

    pub(crate) fn acquire_scratch(&mut self) -> CubeRenderTarget {
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
        filter_cursors: &mut FilterCursors,
    ) {
        match filter {
            Filter::Chain { filters, .. } => {
                for filter in filters {
                    self.apply_filter(target, bounds, filter, filter_cursors);
                }
            }
            Filter::Graph { primitives, .. } => {
                self.apply_filter_graph(target, bounds, primitives, filter_cursors);
            }
            Filter::Blur { radius_x, radius_y } => {
                if radius_x.max(*radius_y).max(0.0) > 0.0 {
                    let temp = self.acquire_scratch();
                    self.blur_buffer(target, temp, bounds, *radius_x, *radius_y);
                    self.release_scratch(temp);
                }
            }
            Filter::ColorMatrix(matrix) => self.apply_color_matrix_filter(target, bounds, *matrix),
            Filter::ComponentTransfer(_) => self.apply_component_transfer_filter(
                target,
                bounds,
                next_filter_transfer_index(&mut filter_cursors.transfer),
            ),
            Filter::ConvolveMatrix(matrix) => {
                let temp = self.acquire_scratch();
                self.clear_buffer(temp, 0);
                self.convolve_matrix_buffer(
                    target,
                    temp,
                    bounds,
                    matrix,
                    next_filter_convolve_offset(matrix, &mut filter_cursors.convolve),
                );
                self.copy_region(temp, target, bounds);
                self.release_scratch(temp);
            }
            Filter::DiffuseLighting(lighting) => {
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.copy_region(target, source, bounds);
                self.diffuse_lighting_buffer(source, target, bounds, lighting);
                self.release_scratch(source);
            }
            Filter::SpecularLighting(lighting) => {
                let source = self.acquire_scratch();
                self.clear_buffer(source, 0);
                self.copy_region(target, source, bounds);
                self.specular_lighting_buffer(source, target, bounds, lighting);
                self.release_scratch(source);
            }
            Filter::Flood { .. } => self.apply_flood(
                target,
                bounds,
                next_filter_brush_index(&mut filter_cursors.brush),
            ),
            Filter::Offset { dx, dy } => {
                let dx = dx.round() as i32;
                let dy = dy.round() as i32;
                if dx != 0 || dy != 0 {
                    let temp = self.acquire_scratch();
                    self.clear_buffer(temp, 0);
                    self.offset_buffer(target, temp, bounds, dx, dy);
                    self.copy_region(temp, target, bounds);
                    self.release_scratch(temp);
                }
            }
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => {
                if (*radius_x).max(*radius_y).max(0.0) > 0.0 {
                    let temp = self.acquire_scratch();
                    let output = self.acquire_scratch();
                    self.clear_buffer(temp, 0);
                    self.clear_buffer(output, 0);
                    self.morphology_buffer(
                        target,
                        temp,
                        output,
                        MorphologyPass {
                            bounds,
                            radius_x: *radius_x,
                            radius_y: *radius_y,
                            operator: *operator,
                        },
                    );
                    self.copy_region(output, target, bounds);
                    self.release_scratch(output);
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
                next_filter_brush_index(&mut filter_cursors.brush),
            ),
            _ => {
                let (filter_kind, amount) = encode_color_filter(filter);
                self.apply_color_filter(target, bounds, filter_kind, amount);
            }
        }
    }

    fn apply_filter_graph(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        primitives: &[FilterPrimitive],
        filter_cursors: &mut FilterCursors,
    ) {
        if primitives.is_empty() {
            self.clear_region(target, bounds);
            return;
        }

        let mut source_alpha = None;
        let mut outputs = Vec::with_capacity(primitives.len());
        for primitive in primitives {
            let output = self.apply_filter_graph_primitive(
                target,
                bounds,
                primitive,
                &outputs,
                &mut source_alpha,
                filter_cursors,
            );
            outputs.push(output);
        }

        let final_output = *outputs
            .last()
            .expect("filter graph should have produced a final output");
        self.clear_region(target, bounds);
        self.copy_region(final_output, target, bounds);

        for output in outputs {
            self.release_scratch(output);
        }
        if let Some(source_alpha) = source_alpha {
            self.release_scratch(source_alpha);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_filter_graph_primitive(
        &mut self,
        source_graphic: CubeRenderTarget,
        bounds: Bounds,
        primitive: &FilterPrimitive,
        outputs: &[CubeRenderTarget],
        source_alpha: &mut Option<CubeRenderTarget>,
        filter_cursors: &mut FilterCursors,
    ) -> CubeRenderTarget {
        let region = primitive.region.intersect(bounds);
        match &primitive.kind {
            FilterPrimitiveKind::Image { .. } => {
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                self.apply_flood(
                    output,
                    region,
                    next_filter_brush_index(&mut filter_cursors.brush),
                );
                output
            }
            FilterPrimitiveKind::Identity => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                );
                self.copy_filter_graph_region(input, bounds, region)
            }
            FilterPrimitiveKind::Filter(filter) => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let temp = self.acquire_scratch();
                self.clear_buffer(temp, 0);
                self.copy_region(input, temp, bounds);
                self.apply_filter(temp, bounds, filter, filter_cursors);
                let output = self.copy_filter_graph_region(temp, bounds, region);
                self.release_scratch(temp);
                output
            }
            FilterPrimitiveKind::Blend { mode } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                self.blend_filter_inputs(input, input2, output, region, *mode);
                output
            }
            FilterPrimitiveKind::Composite { operator } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                self.composite_filter_inputs(input, input2, output, region, *operator);
                output
            }
            FilterPrimitiveKind::Merge { inputs } => {
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                for input in inputs {
                    let input = self.resolve_filter_graph_input(
                        source_graphic,
                        *input,
                        outputs,
                        source_alpha,
                        bounds,
                    );
                    self.source_over_filter_input(input, output, region);
                }
                output
            }
        }
    }

    fn resolve_required_filter_graph_input(
        &mut self,
        source_graphic: CubeRenderTarget,
        primitive: &FilterPrimitive,
        outputs: &[CubeRenderTarget],
        source_alpha: &mut Option<CubeRenderTarget>,
        bounds: Bounds,
    ) -> CubeRenderTarget {
        self.resolve_filter_graph_input(
            source_graphic,
            primitive
                .input2
                .expect("dual-input filter primitive is missing input2"),
            outputs,
            source_alpha,
            bounds,
        )
    }

    fn resolve_filter_graph_input(
        &mut self,
        source_graphic: CubeRenderTarget,
        input: FilterInput,
        outputs: &[CubeRenderTarget],
        source_alpha: &mut Option<CubeRenderTarget>,
        bounds: Bounds,
    ) -> CubeRenderTarget {
        match input {
            FilterInput::SourceGraphic => source_graphic,
            FilterInput::Primitive(index) => outputs[index],
            FilterInput::SourceAlpha => {
                if let Some(target) = *source_alpha {
                    return target;
                }
                let alpha = self.acquire_scratch();
                self.clear_buffer(alpha, 0);
                self.build_source_alpha(source_graphic, alpha, bounds);
                *source_alpha = Some(alpha);
                alpha
            }
        }
    }

    fn copy_filter_graph_region(
        &mut self,
        input: CubeRenderTarget,
        bounds: Bounds,
        region: Bounds,
    ) -> CubeRenderTarget {
        let output = self.acquire_scratch();
        self.clear_buffer(output, 0);
        self.copy_region(input, output, region.intersect(bounds));
        output
    }

    pub(crate) fn clear_buffer(&mut self, target: CubeRenderTarget, clear_color: u32) {
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

    fn clear_region(&mut self, target: CubeRenderTarget, bounds: Bounds) {
        match target {
            CubeRenderTarget::Main => {
                FilterPipeline::clear_region(&self.client, &mut self.target, self.size, bounds)
            }
            CubeRenderTarget::Scratch(ix) => {
                FilterPipeline::clear_region(&self.client, &mut self.scratch[ix], self.size, bounds)
            }
        }
    }

    fn build_source_alpha(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
    ) {
        if source == target {
            return;
        }
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::source_alpha_region(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::source_alpha_region(
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
                FilterPipeline::source_alpha_region(&self.client, source, target, self.size, bounds)
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
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

    fn apply_color_matrix_filter(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        match target {
            CubeRenderTarget::Main => FilterPipeline::apply_color_matrix(
                &self.client,
                &mut self.target,
                self.size,
                bounds,
                matrix,
            ),
            CubeRenderTarget::Scratch(ix) => FilterPipeline::apply_color_matrix(
                &self.client,
                &mut self.scratch[ix],
                self.size,
                bounds,
                matrix,
            ),
        }
    }

    fn apply_component_transfer_filter(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        table_index: u32,
    ) {
        match target {
            CubeRenderTarget::Main => FilterPipeline::apply_component_transfer(
                &self.client,
                &mut self.target,
                self.size,
                bounds,
                table_index,
                &self.filter_transfers.tables,
            ),
            CubeRenderTarget::Scratch(ix) => FilterPipeline::apply_component_transfer(
                &self.client,
                &mut self.scratch[ix],
                self.size,
                bounds,
                table_index,
                &self.filter_transfers.tables,
            ),
        }
    }

    fn convolve_matrix_buffer(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        matrix: &ConvolveMatrix,
        kernel_offset: u32,
    ) {
        if source == target {
            return;
        }
        let edge_mode = encode_convolve_edge_mode(matrix.edge_mode);
        let preserve_alpha = u32::from(matrix.preserve_alpha);
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::convolve_matrix_region(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                    &self.filter_convolves.kernels,
                    kernel_offset,
                    matrix.columns,
                    matrix.rows,
                    matrix.target_x,
                    matrix.target_y,
                    matrix.divisor,
                    matrix.bias,
                    edge_mode,
                    preserve_alpha,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::convolve_matrix_region(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                    &self.filter_convolves.kernels,
                    kernel_offset,
                    matrix.columns,
                    matrix.rows,
                    matrix.target_x,
                    matrix.target_y,
                    matrix.divisor,
                    matrix.bias,
                    edge_mode,
                    preserve_alpha,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::convolve_matrix_region(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    &self.filter_convolves.kernels,
                    kernel_offset,
                    matrix.columns,
                    matrix.rows,
                    matrix.target_x,
                    matrix.target_y,
                    matrix.divisor,
                    matrix.bias,
                    edge_mode,
                    preserve_alpha,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn diffuse_lighting_buffer(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        lighting: &DiffuseLighting,
    ) {
        self.lighting_buffer(
            source,
            target,
            bounds,
            LightingDispatch {
                output_kind: 0,
                surface_scale: lighting.surface_scale,
                light_constant: lighting.diffuse_constant,
                specular_exponent: 1.0,
                lighting_color: lighting.lighting_color,
                light_source: lighting.light_source,
            },
        );
    }

    fn specular_lighting_buffer(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        lighting: &SpecularLighting,
    ) {
        self.lighting_buffer(
            source,
            target,
            bounds,
            LightingDispatch {
                output_kind: 1,
                surface_scale: lighting.surface_scale,
                light_constant: lighting.specular_constant,
                specular_exponent: lighting.specular_exponent,
                lighting_color: lighting.lighting_color,
                light_source: lighting.light_source,
            },
        );
    }

    fn lighting_buffer(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        lighting: LightingDispatch,
    ) {
        if source == target {
            return;
        }
        let light_kind = encode_light_source_kind(lighting.light_source);
        let params = light_source_params(lighting.light_source);
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::lighting_region(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                    lighting.output_kind,
                    lighting.surface_scale,
                    lighting.light_constant,
                    lighting.specular_exponent,
                    lighting.lighting_color,
                    light_kind,
                    params,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::lighting_region(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                    lighting.output_kind,
                    lighting.surface_scale,
                    lighting.light_constant,
                    lighting.specular_exponent,
                    lighting.lighting_color,
                    light_kind,
                    params,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::lighting_region(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    lighting.output_kind,
                    lighting.surface_scale,
                    lighting.light_constant,
                    lighting.specular_exponent,
                    lighting.lighting_color,
                    light_kind,
                    params,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn apply_flood(&mut self, target: CubeRenderTarget, bounds: Bounds, brush_index: u32) {
        match target {
            CubeRenderTarget::Main => {
                let brushes = self.filter_brushes.resources();
                FilterPipeline::flood_region(
                    &self.client,
                    &mut self.target,
                    self.size,
                    bounds,
                    brush_index,
                    brushes,
                )
            }
            CubeRenderTarget::Scratch(ix) => {
                let brushes = self.filter_brushes.resources();
                FilterPipeline::flood_region(
                    &self.client,
                    &mut self.scratch[ix],
                    self.size,
                    bounds,
                    brush_index,
                    brushes,
                )
            }
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

    fn blend_filter_inputs(
        &mut self,
        input1: CubeRenderTarget,
        input2: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        mode: Mix,
    ) {
        let size = self.size;
        let mode = encode_blend_mode(BlendMode::new(mode, Compose::SrcOver));
        self.dual_input_filter(
            input1,
            input2,
            target,
            move |client, input1, input2, target| {
                FilterPipeline::blend_region(client, input1, input2, target, size, bounds, mode)
            },
        );
    }

    fn composite_filter_inputs(
        &mut self,
        input1: CubeRenderTarget,
        input2: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        operator: CompositeOperator,
    ) {
        let size = self.size;
        let operator_code = encode_composite_operator(operator);
        let arithmetic = composite_arithmetic(operator);
        self.dual_input_filter(
            input1,
            input2,
            target,
            move |client, input1, input2, target| {
                FilterPipeline::composite_inputs_region(
                    client,
                    input1,
                    input2,
                    target,
                    size,
                    bounds,
                    operator_code,
                    arithmetic,
                )
            },
        );
    }

    fn source_over_filter_input(
        &mut self,
        input: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
    ) {
        let CubeRenderTarget::Scratch(target_ix) = target else {
            panic!("merge graph primitives must write to scratch output");
        };
        match input {
            CubeRenderTarget::Main => FilterPipeline::source_over_region(
                &self.client,
                &self.target,
                &mut self.scratch[target_ix],
                self.size,
                bounds,
            ),
            CubeRenderTarget::Scratch(source_ix) => {
                let (target, source) =
                    scratch_target_and_source(&mut self.scratch, target_ix, source_ix);
                FilterPipeline::source_over_region(&self.client, source, target, self.size, bounds);
            }
        }
    }

    fn dual_input_filter(
        &mut self,
        input1: CubeRenderTarget,
        input2: CubeRenderTarget,
        target: CubeRenderTarget,
        run: impl FnOnce(
            &::cubecl::client::ComputeClient<R>,
            &CubeBuffer<u32>,
            &CubeBuffer<u32>,
            &mut CubeBuffer<u32>,
        ),
    ) {
        let CubeRenderTarget::Scratch(target_ix) = target else {
            panic!("dual-input graph primitives must write to scratch output");
        };
        match (input1, input2) {
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => {
                run(
                    &self.client,
                    &self.target,
                    &self.target,
                    &mut self.scratch[target_ix],
                );
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(b)) => {
                let (target, input2) = scratch_target_and_source(&mut self.scratch, target_ix, b);
                run(&self.client, &self.target, input2, target);
            }
            (CubeRenderTarget::Scratch(a), CubeRenderTarget::Main) => {
                let (target, input1) = scratch_target_and_source(&mut self.scratch, target_ix, a);
                run(&self.client, input1, &self.target, target);
            }
            (CubeRenderTarget::Scratch(a), CubeRenderTarget::Scratch(b)) => {
                let (target, input1, input2) =
                    scratch_target_and_two_sources(&mut self.scratch, target_ix, a, b);
                run(&self.client, input1, input2, target);
            }
        }
    }

    fn offset_buffer(
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
                FilterPipeline::offset_region(
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
                FilterPipeline::offset_region(
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
                FilterPipeline::offset_region(
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

    fn morphology_buffer(
        &mut self,
        source: CubeRenderTarget,
        temp: CubeRenderTarget,
        target: CubeRenderTarget,
        pass: MorphologyPass,
    ) {
        let raw_radius_x = pass.radius_x.max(0.0).ceil() as u32;
        let raw_radius_y = pass.radius_y.max(0.0).ceil() as u32;
        if source == target
            || source == temp
            || temp == target
            || (raw_radius_x == 0 && raw_radius_y == 0)
        {
            return;
        }

        if pass.operator == MorphologyOperator::Erode
            && (raw_radius_x.saturating_mul(2) >= self.size.0
                || raw_radius_y.saturating_mul(2) >= self.size.1)
        {
            self.clear_buffer(target, 0);
            return;
        }

        let radius_x = raw_radius_x.min(self.size.0.saturating_sub(1));
        let radius_y = raw_radius_y.min(self.size.1.saturating_sub(1));
        let operator = encode_morphology_operator(pass.operator);
        self.morphology_axis_buffer(source, temp, pass.bounds, radius_x, operator, 0);
        self.morphology_axis_buffer(temp, target, pass.bounds, radius_y, operator, 1);
    }

    fn morphology_axis_buffer(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) {
        if source == target {
            return;
        }
        match (source, target) {
            (CubeRenderTarget::Main, CubeRenderTarget::Scratch(target_ix)) => {
                FilterPipeline::morphology_axis_region(
                    &self.client,
                    &self.target,
                    &mut self.scratch[target_ix],
                    self.size,
                    bounds,
                    radius,
                    operator,
                    axis,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Main) => {
                FilterPipeline::morphology_axis_region(
                    &self.client,
                    &self.scratch[source_ix],
                    &mut self.target,
                    self.size,
                    bounds,
                    radius,
                    operator,
                    axis,
                )
            }
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::morphology_axis_region(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    radius,
                    operator,
                    axis,
                )
            }
            (CubeRenderTarget::Main, CubeRenderTarget::Main) => unreachable!(),
        }
    }

    fn blur_buffer(
        &mut self,
        target: CubeRenderTarget,
        temp: CubeRenderTarget,
        bounds: Bounds,
        radius_x: f32,
        radius_y: f32,
    ) {
        let radius_x = radius_x.max(0.0);
        let radius_y = radius_y.max(0.0);
        if radius_x <= 0.0 && radius_y <= 0.0 {
            return;
        }
        // Keep SVG's independent X/Y blur semantics: single-axis blur writes
        // into scratch first, then copies the completed pass back to target.
        match (radius_x > 0.0, radius_y > 0.0) {
            (true, true) => {
                self.blur_pass(target, temp, bounds, radius_x, 0);
                self.blur_pass(temp, target, bounds, radius_y, 1);
            }
            (true, false) => {
                self.blur_pass(target, temp, bounds, radius_x, 0);
                self.copy_region(temp, target, bounds);
            }
            (false, true) => {
                self.blur_pass(target, temp, bounds, radius_y, 1);
                self.copy_region(temp, target, bounds);
            }
            (false, false) => {}
        }
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
            self.blur_buffer(shadow, temp, bounds, radius, radius);
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

    fn composite_blend_with_stack(
        &mut self,
        target: CubeRenderTarget,
        source: CubeRenderTarget,
        mask: CubeRenderTarget,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        mode: BlendMode,
    ) {
        let mode = encode_blend_mode(mode);
        match (target, source, mask) {
            (
                CubeRenderTarget::Main,
                CubeRenderTarget::Scratch(source_ix),
                CubeRenderTarget::Scratch(mask_ix),
            ) => FilterPipeline::composite_blend_stack_region(
                &self.client,
                &self.scene,
                &self.scan,
                &mut self.target,
                &self.scratch[source_ix],
                &self.scratch[mask_ix],
                self.size,
                bounds,
                layer_stack.start as u32,
                layer_stack.end as u32,
                self.max_group_depth,
                mode,
            ),
            (
                CubeRenderTarget::Scratch(target_ix),
                CubeRenderTarget::Scratch(source_ix),
                CubeRenderTarget::Scratch(mask_ix),
            ) => {
                let (target, source, mask) =
                    scratch_target_source_mask(&mut self.scratch, target_ix, source_ix, mask_ix);
                FilterPipeline::composite_blend_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    target,
                    source,
                    mask,
                    self.size,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                    mode,
                )
            }
            _ => panic!("CubeCL blend stack composite requires scratch source and scratch mask"),
        }
    }

    pub(crate) fn build_region_mask(
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

    pub(crate) fn build_layer_mask(&mut self, target: CubeRenderTarget, draw: u32, bounds: Bounds) {
        let CubeRenderTarget::Scratch(target_ix) = target else {
            panic!("CubeCL layer masks must be rendered into preallocated scratch");
        };
        FilterPipeline::rasterize_layer_mask(
            &self.client,
            &self.scene,
            &self.scan,
            &mut self.scratch[target_ix],
            self.size,
            bounds,
            draw,
        );
    }

    fn svg_mask_coverage(
        &mut self,
        source: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        kind: MaskKind,
    ) {
        let kind = encode_mask_kind(kind);
        match (source, target) {
            (CubeRenderTarget::Scratch(source_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (source, target) =
                    scratch_source_target(&mut self.scratch, source_ix, target_ix);
                FilterPipeline::svg_mask_coverage_region(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    kind,
                );
            }
            _ => panic!("CubeCL SVG mask coverage requires scratch source and target"),
        }
    }

    fn apply_region_mask(
        &mut self,
        mask: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
    ) {
        match (mask, target) {
            (CubeRenderTarget::Scratch(mask_ix), CubeRenderTarget::Scratch(target_ix)) => {
                let (mask, target) = scratch_source_target(&mut self.scratch, mask_ix, target_ix);
                FilterPipeline::apply_region_mask(&self.client, mask, target, self.size, bounds);
            }
            _ => panic!("CubeCL region mask application requires scratch buffers"),
        }
    }
}

fn draw_bounds(scene: &Scene, draw_ix: usize) -> Bounds {
    let bounds = scene.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

pub(super) fn plan_stack_depths(plan: &ExecPlan) -> (usize, usize) {
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
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[outer_stack.clone()]);
                let (content_clip_depth, content_group_depth) =
                    plan_stack_depths_for_ops(content, plan);
                let (mask_clip_depth, mask_group_depth) = plan_stack_depths_for_ops(mask, plan);
                max_clip_depth = max_clip_depth
                    .max(clip_depth)
                    .max(content_clip_depth)
                    .max(mask_clip_depth);
                max_group_depth = max_group_depth
                    .max(group_depth)
                    .max(content_group_depth)
                    .max(mask_group_depth);
            }
            _ => {}
        }
    }
    (max_clip_depth, max_group_depth)
}

fn encode_mask_kind(kind: MaskKind) -> u32 {
    match kind {
        MaskKind::Alpha => SVG_MASK_ALPHA,
        MaskKind::Luminance => SVG_MASK_LUMINANCE,
    }
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

pub(super) fn required_scratch_count(plan: &ExecPlan) -> usize {
    max_scratch_for_ops(&plan.ops, 0)
}

fn max_scratch_for_ops(ops: &[ExecOp], held: usize) -> usize {
    let mut max_count = held;
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer,
                outer_stack,
                children,
                ..
            } => match layer {
                Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + 1);
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
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
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                let content_held = held + 1;
                max_count = max_count.max(max_scratch_for_ops(content, content_held));
                let mask_source_held = held + 2;
                max_count = max_count.max(mask_source_held + 1);
                max_count = max_count.max(max_scratch_for_ops(mask, mask_source_held));
            }
            _ => {}
        }
    }
    max_count
}

fn filter_scratch_extra(filter: &Filter) -> usize {
    match filter {
        Filter::Chain { filters, .. } => {
            filters.iter().map(filter_scratch_extra).max().unwrap_or(0)
        }
        Filter::Graph { primitives, .. } => graph_scratch_extra(primitives),
        Filter::Blur { radius_x, radius_y } => usize::from(radius_x.max(*radius_y) > 0.0),
        Filter::ConvolveMatrix(_) => 1,
        Filter::DiffuseLighting(_) => 1,
        Filter::SpecularLighting(_) => 1,
        Filter::Offset { .. } => 1,
        Filter::Morphology { .. } => 2,
        Filter::DropShadow { radius, .. } => 1 + usize::from(radius.max(0.0) > 0.0),
        _ => 0,
    }
}

fn graph_scratch_extra(primitives: &[FilterPrimitive]) -> usize {
    let source_alpha = primitives.iter().any(|primitive| {
        primitive.input == FilterInput::SourceAlpha
            || primitive.input2 == Some(FilterInput::SourceAlpha)
    });
    let unary_temp = primitives
        .iter()
        .filter_map(|primitive| match &primitive.kind {
            FilterPrimitiveKind::Filter(filter) => Some(1 + filter_scratch_extra(filter)),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    primitives.len() + usize::from(source_alpha) + unary_temp
}

pub(crate) fn encode_layer_payload(entry: LayerStackEntry) -> u32 {
    match entry {
        LayerStackEntry::Clip { .. } => 0,
        LayerStackEntry::Opacity { opacity, .. } => opacity_f32_to_u8(opacity) as u32,
        LayerStackEntry::Blend { mode, .. } => encode_blend_mode(mode),
    }
}

fn encode_blend_mode(mode: BlendMode) -> u32 {
    mode.mix as u32 | ((mode.compose as u32) << 8)
}

fn encode_composite_operator(operator: CompositeOperator) -> u32 {
    match operator {
        CompositeOperator::Over => 0,
        CompositeOperator::In => 1,
        CompositeOperator::Out => 2,
        CompositeOperator::Atop => 3,
        CompositeOperator::Xor => 4,
        CompositeOperator::Arithmetic { .. } => 5,
    }
}

fn encode_morphology_operator(operator: MorphologyOperator) -> u32 {
    match operator {
        MorphologyOperator::Erode => 0,
        MorphologyOperator::Dilate => 1,
    }
}

fn encode_convolve_edge_mode(edge_mode: ConvolveEdgeMode) -> u32 {
    match edge_mode {
        ConvolveEdgeMode::None => 0,
        ConvolveEdgeMode::Duplicate => 1,
        ConvolveEdgeMode::Wrap => 2,
    }
}

fn encode_light_source_kind(light_source: LightSource) -> u32 {
    match light_source {
        LightSource::Distant { .. } => 0,
        LightSource::Point { .. } => 1,
        LightSource::Spot { .. } => 2,
    }
}

fn light_source_params(light_source: LightSource) -> [f32; 9] {
    match light_source {
        LightSource::Distant { azimuth, elevation } => {
            [azimuth, elevation, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0]
        }
        LightSource::Point { x, y, z } => [x, y, z, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0],
        LightSource::Spot {
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle,
        } => [
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle.unwrap_or(-1.0),
            0.0,
        ],
    }
}

fn composite_arithmetic(operator: CompositeOperator) -> [f32; 4] {
    match operator {
        CompositeOperator::Arithmetic { k1, k2, k3, k4 } => [k1, k2, k3, k4],
        _ => [0.0; 4],
    }
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
        Filter::Blur { .. } => panic!("blur is handled by CubeCL separable blur passes"),
        Filter::ColorMatrix(_) => panic!("color matrix is handled by a dedicated CubeCL pass"),
        Filter::ComponentTransfer(_) => {
            panic!("component transfer is handled by a dedicated CubeCL pass")
        }
        Filter::ConvolveMatrix(_) => {
            panic!("convolve matrix is handled by a dedicated CubeCL pass")
        }
        Filter::DiffuseLighting(_) => {
            panic!("diffuse lighting is handled by a dedicated CubeCL pass")
        }
        Filter::SpecularLighting(_) => {
            panic!("specular lighting is handled by a dedicated CubeCL pass")
        }
        Filter::Graph { .. } => panic!("filter graphs are handled by CubeCL graph execution"),
        Filter::Flood { .. } => panic!("flood is handled by the CubeCL brush fill pass"),
        Filter::Offset { .. } => panic!("offset is handled by a dedicated CubeCL pass"),
        Filter::Morphology { .. } => panic!("morphology is handled by a dedicated CubeCL pass"),
        Filter::DropShadow { .. } => {
            panic!("drop-shadow is handled by the CubeCL shadow-mask passes")
        }
        Filter::Chain { .. } => panic!("filter chains are expanded before CubeCL filter encoding"),
    }
}

fn supported_filter_bounds(filter: &Filter, sample_region: &Region, size: (u32, u32)) -> Bounds {
    let bounds = region_bounds(sample_region);
    let outset = filter_outset(filter);
    bounds
        .outset(outset)
        .intersect(Bounds::canvas(size.0, size.1))
}

fn filter_outset(filter: &Filter) -> i32 {
    match filter {
        Filter::Chain {
            filters,
            fixed_region,
        } => {
            if *fixed_region {
                0
            } else {
                filters.iter().map(filter_outset).sum()
            }
        }
        Filter::Blur { radius_x, radius_y } => blur_outset(radius_x.max(*radius_y)),
        Filter::Offset { dx, dy } => dx.abs().ceil().max(dy.abs().ceil()) as i32,
        Filter::Morphology {
            radius_x,
            radius_y,
            operator,
        } => match operator {
            MorphologyOperator::Erode => 0,
            MorphologyOperator::Dilate => (*radius_x).max(*radius_y).max(0.0).ceil() as i32,
        },
        Filter::DropShadow {
            radius,
            offset_x,
            offset_y,
            ..
        } => blur_outset(*radius) + offset_x.abs().ceil().max(offset_y.abs().ceil()) as i32,
        _ => 0,
    }
}

fn next_filter_brush_index(cursor: &mut usize) -> u32 {
    let index = *cursor as u32;
    *cursor += 1;
    index
}

fn next_filter_transfer_index(cursor: &mut usize) -> u32 {
    let index = *cursor as u32;
    *cursor += 1;
    index
}

fn next_filter_convolve_offset(matrix: &ConvolveMatrix, cursor: &mut usize) -> u32 {
    let offset = *cursor as u32;
    *cursor += matrix.data.len();
    offset
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

pub(super) struct FilterTransferBuffers {
    pub(crate) tables: CubeBuffer<u32>,
}

pub(super) struct FilterConvolveBuffers {
    pub(crate) kernels: CubeBuffer<f32>,
}

impl FilterConvolveBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            kernels: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        upload: FilterConvolveUpload,
    ) {
        self.kernels.replace(client, &upload.kernels);
    }
}

#[derive(Default)]
pub(super) struct FilterConvolveUpload {
    kernels: Vec<f32>,
}

impl FilterConvolveUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_convolves_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_matrix(&mut self, matrix: &ConvolveMatrix) {
        self.kernels.extend_from_slice(&matrix.data);
    }
}

impl FilterTransferBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            tables: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        upload: FilterTransferUpload,
    ) {
        self.tables.replace(client, &upload.tables);
    }
}

#[derive(Default)]
pub(super) struct FilterTransferUpload {
    tables: Vec<u32>,
}

impl FilterTransferUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
        let mut upload = Self::default();
        collect_filter_transfers_for_ops(&plan.ops, &mut upload);
        upload
    }

    fn push_table(&mut self, table: &ComponentTransferTable) {
        self.tables.extend_from_slice(table);
    }
}

pub(super) struct FilterPathBuffers {
    pub(crate) range_starts: CubeBuffer<u32>,
    pub(crate) range_ends: CubeBuffer<u32>,
    pub(crate) p0x: CubeBuffer<i32>,
    pub(crate) p0y: CubeBuffer<i32>,
    pub(crate) p1x: CubeBuffer<i32>,
    pub(crate) p1y: CubeBuffer<i32>,
}

impl FilterPathBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            range_starts: CubeBuffer::new(client, 0),
            range_ends: CubeBuffer::new(client, 0),
            p0x: CubeBuffer::new(client, 0),
            p0y: CubeBuffer::new(client, 0),
            p1x: CubeBuffer::new(client, 0),
            p1y: CubeBuffer::new(client, 0),
        }
    }

    pub(super) fn upload<R: Runtime>(
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
pub(super) struct FilterPathUpload {
    range_starts: Vec<u32>,
    range_ends: Vec<u32>,
    p0x: Vec<i32>,
    p0y: Vec<i32>,
    p1x: Vec<i32>,
    p1y: Vec<i32>,
}

impl FilterPathUpload {
    pub(super) fn from_plan(plan: &ExecPlan) -> Self {
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
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. } => {
                    upload.push_region(sample_region);
                    collect_filter_paths_for_ops(children, upload);
                }
                _ => collect_filter_paths_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer {
                layer,
                content,
                mask,
                ..
            } => {
                upload.push_region(&layer.region);
                collect_filter_paths_for_ops(content, upload);
                collect_filter_paths_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_transfers_for_ops(ops: &[ExecOp], upload: &mut FilterTransferUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_transfers_for_ops(children, upload);
                    collect_filter_transfer(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_transfer(filter, upload);
                    collect_filter_transfers_for_ops(children, upload);
                }
                _ => collect_filter_transfers_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_transfers_for_ops(content, upload);
                collect_filter_transfers_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_convolves_for_ops(ops: &[ExecOp], upload: &mut FilterConvolveUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_convolves_for_ops(children, upload);
                    collect_filter_convolve(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_convolve(filter, upload);
                    collect_filter_convolves_for_ops(children, upload);
                }
                _ => collect_filter_convolves_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_convolves_for_ops(content, upload);
                collect_filter_convolves_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_convolve(filter: &Filter, upload: &mut FilterConvolveUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_convolve(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                if let FilterPrimitiveKind::Filter(filter) = &primitive.kind {
                    collect_filter_convolve(filter, upload);
                }
            }
        }
        Filter::ConvolveMatrix(matrix) => upload.push_matrix(matrix),
        _ => {}
    }
}

fn collect_filter_transfer(filter: &Filter, upload: &mut FilterTransferUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_transfer(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                if let FilterPrimitiveKind::Filter(filter) = &primitive.kind {
                    collect_filter_transfer(filter, upload);
                }
            }
        }
        Filter::ComponentTransfer(table) => upload.push_table(table),
        _ => {}
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

fn scratch_target_and_two_sources(
    scratch: &mut [CubeBuffer<u32>],
    target_ix: usize,
    source_a_ix: usize,
    source_b_ix: usize,
) -> (&mut CubeBuffer<u32>, &CubeBuffer<u32>, &CubeBuffer<u32>) {
    assert_ne!(
        target_ix, source_a_ix,
        "target and first source scratch buffers must differ"
    );
    assert_ne!(
        target_ix, source_b_ix,
        "target and second source scratch buffers must differ"
    );

    let (before, target_and_after) = scratch.split_at_mut(target_ix);
    let (target_slice, after) = target_and_after.split_at_mut(1);
    let target = &mut target_slice[0];
    let source_a = scratch_ref_except_target(before, after, target_ix, source_a_ix);
    let source_b = scratch_ref_except_target(before, after, target_ix, source_b_ix);
    (target, source_a, source_b)
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
