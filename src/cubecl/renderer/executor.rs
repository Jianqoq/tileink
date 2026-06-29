use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, Compose, Mix, kurbo::Shape};

use crate::{
    cpu::line_scanned_tile_count,
    scene::Scene,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        layer::{
            Layer,
            filter::{
                self as filter_model, ColorChannel, CompositeOperator, ConvolveEdgeMode,
                ConvolveMatrix, DiffuseLighting, DisplacementMap, Filter, FilterInput,
                FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
                SpecularLighting, Turbulence, TurbulenceKind, filter_offset_to_pixel_delta,
            },
            mask::MaskKind,
            region::Region,
        },
        offscreen::{local_filter, local_offscreen_scene},
        pixel::opacity_f32_to_u8,
    },
};

use crate::cubecl::{
    brush::{GpuBrushBuffers, GpuBrushUpload},
    buffer::CubeBuffer,
    pipelines::{
        cumsum::CumsumPipeline,
        filter::{
            FILTER_BRIGHTNESS, FILTER_CONTRAST, FILTER_GRAYSCALE, FILTER_HUE_ROTATE, FILTER_INVERT,
            FILTER_OPACITY, FILTER_SATURATE, FILTER_SEPIA, FilterPipeline, SVG_MASK_ALPHA,
            SVG_MASK_LUMINANCE,
        },
        fine::FinePipeline,
        scan::ScanPipeline,
    },
    renderer::{CoarseBuffers, ScanBuffers, SceneBuffers},
    types::CubeBufferLengths,
};

use super::{
    Renderer,
    filter_cursors::FilterCursors,
    filter_resources::{
        FilterConvolveBuffers, FilterConvolveUpload, FilterPathBuffers, FilterPathUpload,
        FilterTransferBuffers, FilterTransferUpload, FilterTurbulenceBuffers,
        FilterTurbulenceUpload,
    },
    resources::SceneUploadStaging,
    scratch::{filter_scratch_extra, plan_stack_depths, required_scratch_count},
    target::CubeRenderTarget,
};

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

struct SavedRendererState {
    size: (u32, u32),
    surface_origin: (i32, i32),
    lengths: CubeBufferLengths,
    max_clip_depth: usize,
    max_group_depth: usize,
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
    scratch: Vec<CubeBuffer<u32>>,
    scratch_in_use: Vec<bool>,
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

    fn activate_local_scene_resources(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        parent_filter: &Filter,
        scratch_count: usize,
        surface_origin: (i32, i32),
    ) -> SavedRendererState {
        let saved = SavedRendererState {
            size: self.size,
            surface_origin: self.surface_origin,
            lengths: self.lengths,
            max_clip_depth: self.max_clip_depth,
            max_group_depth: self.max_group_depth,
            scene: std::mem::replace(&mut self.scene, SceneBuffers::new(&self.client)),
            scan: std::mem::replace(&mut self.scan, ScanBuffers::new(&self.client)),
            coarse: std::mem::replace(&mut self.coarse, CoarseBuffers::new(&self.client)),
            scene_upload: std::mem::take(&mut self.scene_upload),
            draw_brushes: std::mem::replace(
                &mut self.draw_brushes,
                GpuBrushBuffers::new(&self.client),
            ),
            filter_brushes: std::mem::replace(
                &mut self.filter_brushes,
                GpuBrushBuffers::new(&self.client),
            ),
            filter_convolves: std::mem::replace(
                &mut self.filter_convolves,
                FilterConvolveBuffers::new(&self.client),
            ),
            filter_paths: std::mem::replace(
                &mut self.filter_paths,
                FilterPathBuffers::new(&self.client),
            ),
            filter_transfers: std::mem::replace(
                &mut self.filter_transfers,
                FilterTransferBuffers::new(&self.client),
            ),
            filter_turbulence: std::mem::replace(
                &mut self.filter_turbulence,
                FilterTurbulenceBuffers::new(&self.client),
            ),
            scratch: std::mem::take(&mut self.scratch),
            scratch_in_use: std::mem::take(&mut self.scratch_in_use),
        };

        let lengths = CubeBufferLengths::from_scene(scene);
        let (max_clip_depth, max_group_depth) = plan_stack_depths(plan);
        self.size = (scene.width, scene.height);
        self.surface_origin = surface_origin;
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.prepare_scratch_buffers(scratch_count.max(1));
        self.draw_brushes
            .upload(&self.client, GpuBrushUpload::from_scene_draws(scene));
        self.filter_brushes.upload(
            &self.client,
            GpuBrushUpload::from_filter_ops_and_filter(&plan.ops, parent_filter),
        );
        self.filter_convolves.upload(
            &self.client,
            FilterConvolveUpload::from_ops_and_filter(&plan.ops, parent_filter),
        );
        self.filter_paths
            .upload(&self.client, FilterPathUpload::from_plan(plan));
        self.filter_transfers.upload(
            &self.client,
            FilterTransferUpload::from_ops_and_filter(&plan.ops, parent_filter),
        );
        self.filter_turbulence.upload(
            &self.client,
            FilterTurbulenceUpload::from_ops_and_filter(&plan.ops, parent_filter),
        );
        self.scene
            .upload(&self.client, scene, plan, &mut self.scene_upload);
        self.scan.prepare_outputs(&self.client, lengths);
        self.coarse.prepare_outputs(&self.client, lengths);
        saved
    }

    fn restore_root_scene_resources(&mut self, saved: SavedRendererState) {
        self.size = saved.size;
        self.surface_origin = saved.surface_origin;
        self.lengths = saved.lengths;
        self.max_clip_depth = saved.max_clip_depth;
        self.max_group_depth = saved.max_group_depth;
        self.scene = saved.scene;
        self.scan = saved.scan;
        self.coarse = saved.coarse;
        self.scene_upload = saved.scene_upload;
        self.draw_brushes = saved.draw_brushes;
        self.filter_brushes = saved.filter_brushes;
        self.filter_convolves = saved.filter_convolves;
        self.filter_paths = saved.filter_paths;
        self.filter_transfers = saved.filter_transfers;
        self.filter_turbulence = saved.filter_turbulence;
        self.scratch = saved.scratch;
        self.scratch_in_use = saved.scratch_in_use;
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
            Layer::Isolate => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw,
                    outer_stack,
                    children,
                    opacity: None,
                    composite: LayerComposite::SrcOver,
                },
                target,
                filter_cursors,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw,
                    outer_stack,
                    children,
                    opacity: Some(opacity.opacity),
                    composite: LayerComposite::SrcOver,
                },
                target,
                filter_cursors,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw,
                    outer_stack,
                    children,
                    opacity: None,
                    composite: LayerComposite::Blend(blend.mode),
                },
                target,
                filter_cursors,
            ),
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer_with_local_surface(
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
            } => {
                let bounds = filter_model::filtered_region_bounds(
                    filter,
                    sample_region,
                    Bounds::canvas(self.size.0, self.size.1),
                );
                let path_index = filter_cursors.next_path_index(sample_region);
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

    fn execute_masked_group_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        group: MaskedGroupLayer<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let bounds =
            draw_bounds(scene, group.draw).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return;
        }

        let source = self.acquire_scratch();
        self.clear_buffer(source, 0);
        self.execute_ops(scene, plan, group.children, source, filter_cursors);
        if let Some(opacity) = group.opacity {
            self.apply_color_filter(source, bounds, FILTER_OPACITY, opacity);
        }

        let mask = self.acquire_scratch();
        self.clear_buffer(mask, 0);
        self.build_layer_mask(mask, group.draw as u32, bounds);
        match group.composite {
            LayerComposite::SrcOver => self.composite_src_over_with_stack(
                target,
                source,
                Some(mask),
                bounds,
                group.outer_stack,
            ),
            LayerComposite::Blend(mode) => self.composite_blend_with_stack(
                target,
                source,
                mask,
                bounds,
                group.outer_stack,
                mode,
            ),
        }
        self.release_scratch(mask);
        self.release_scratch(source);
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_filter_layer_with_local_surface(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        filter: &Filter,
        sample_region: &Region,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let target_bounds = Bounds::canvas(self.size.0, self.size.1);
        let Some(filter_bounds) =
            filter_model::filter_surface_bounds(filter, sample_region, target_bounds)
        else {
            filter_cursors.advance_filter_layer(sample_region, children, filter);
            return;
        };

        filter_cursors.advance_filter_layer(sample_region, children, filter);
        let local = local_offscreen_scene(
            scene,
            plan,
            children,
            filter_bounds.surface,
            line_scanned_tile_count,
        );
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

        let source = CubeRenderTarget::Scratch(0);
        self.scratch_in_use[0] = true;
        self.clear_buffer(source, 0);
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
        let mut local_filter_cursors = FilterCursors::default();
        self.execute_ops(
            &local.scene,
            &local.plan,
            &local.children,
            source,
            &mut local_filter_cursors,
        );
        self.apply_filter(
            source,
            local_bounds,
            &local_filter,
            &mut local_filter_cursors,
        );

        let mut local_scratch = std::mem::take(&mut self.scratch);
        let source_buffer = local_scratch.remove(0);
        self.scratch_in_use.clear();
        self.restore_root_scene_resources(saved);
        self.composite_surface_src_over_with_stack(
            target,
            &source_buffer,
            (
                filter_bounds.surface.width(),
                filter_bounds.surface.height(),
            ),
            (filter_bounds.surface.x0, filter_bounds.surface.y0),
            filter_bounds.output,
            outer_stack,
        );
        self.surface_sources.push(source_buffer);
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
        let path_index = filter_cursors.next_path_index(&layer.region);

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
                filter_cursors.next_transfer_index(),
            ),
            Filter::ConvolveMatrix(matrix) => {
                let temp = self.acquire_scratch();
                self.clear_buffer(temp, 0);
                self.convolve_matrix_buffer(
                    target,
                    temp,
                    bounds,
                    matrix,
                    filter_cursors.next_convolve_offset(matrix),
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
            Filter::Flood { .. } => {
                self.apply_flood(target, bounds, filter_cursors.next_brush_index())
            }
            Filter::Offset { dx, dy } => {
                let dx = filter_offset_to_pixel_delta(*dx);
                let dy = filter_offset_to_pixel_delta(*dy);
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
                filter_cursors.next_brush_index(),
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
                self.apply_flood(output, region, filter_cursors.next_brush_index());
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
            FilterPrimitiveKind::DisplacementMap(displacement) => {
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
                self.displacement_map_filter_inputs(input, input2, output, region, displacement);
                output
            }
            FilterPrimitiveKind::Tile { source_region } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                );
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                self.tile_filter_input(input, output, region, *source_region);
                output
            }
            FilterPrimitiveKind::Turbulence(turbulence) => {
                let output = self.acquire_scratch();
                self.clear_buffer(output, 0);
                self.apply_turbulence(
                    output,
                    region,
                    turbulence,
                    filter_cursors.next_turbulence_index(),
                );
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
                    self.surface_origin,
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
                    self.surface_origin,
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
                    self.surface_origin,
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

    fn apply_turbulence(
        &mut self,
        target: CubeRenderTarget,
        bounds: Bounds,
        turbulence: &Turbulence,
        table_index: u32,
    ) {
        let kind = encode_turbulence_kind(turbulence.kind);
        let stitch_tiles = u32::from(turbulence.stitch_tiles);
        let linear_rgb = u32::from(turbulence.linear_rgb);
        match target {
            CubeRenderTarget::Main => FilterPipeline::turbulence_region(
                &self.client,
                &mut self.target,
                self.size,
                bounds,
                turbulence.base_frequency_x,
                turbulence.base_frequency_y,
                turbulence.num_octaves,
                stitch_tiles,
                kind,
                linear_rgb,
                table_index,
                turbulence.transform_x,
                turbulence.transform_y,
                turbulence.scale_x,
                turbulence.scale_y,
                turbulence.tile_x,
                turbulence.tile_y,
                turbulence.tile_width,
                turbulence.tile_height,
                &self.filter_turbulence.selectors,
                &self.filter_turbulence.gradients,
            ),
            CubeRenderTarget::Scratch(ix) => FilterPipeline::turbulence_region(
                &self.client,
                &mut self.scratch[ix],
                self.size,
                bounds,
                turbulence.base_frequency_x,
                turbulence.base_frequency_y,
                turbulence.num_octaves,
                stitch_tiles,
                kind,
                linear_rgb,
                table_index,
                turbulence.transform_x,
                turbulence.transform_y,
                turbulence.scale_x,
                turbulence.scale_y,
                turbulence.tile_x,
                turbulence.tile_y,
                turbulence.tile_width,
                turbulence.tile_height,
                &self.filter_turbulence.selectors,
                &self.filter_turbulence.gradients,
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

    fn displacement_map_filter_inputs(
        &mut self,
        input1: CubeRenderTarget,
        input2: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        displacement: &DisplacementMap,
    ) {
        let size = self.size;
        let x_channel = encode_color_channel(displacement.x_channel);
        let y_channel = encode_color_channel(displacement.y_channel);
        let linear_rgb = u32::from(displacement.linear_rgb);
        let scale_x = displacement.scale_x;
        let scale_y = displacement.scale_y;
        self.dual_input_filter(
            input1,
            input2,
            target,
            move |client, input1, input2, target| {
                FilterPipeline::displacement_map_region(
                    client, input1, input2, target, size, bounds, scale_x, scale_y, x_channel,
                    y_channel, linear_rgb,
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

    fn tile_filter_input(
        &mut self,
        input: CubeRenderTarget,
        target: CubeRenderTarget,
        bounds: Bounds,
        source_bounds: Bounds,
    ) {
        let CubeRenderTarget::Scratch(target_ix) = target else {
            panic!("tile graph primitives must write to scratch output");
        };
        match input {
            CubeRenderTarget::Main => FilterPipeline::tile_region(
                &self.client,
                &self.target,
                &mut self.scratch[target_ix],
                self.size,
                bounds,
                source_bounds,
            ),
            CubeRenderTarget::Scratch(source_ix) => {
                let (target, source) =
                    scratch_target_and_source(&mut self.scratch, target_ix, source_ix);
                FilterPipeline::tile_region(
                    &self.client,
                    source,
                    target,
                    self.size,
                    bounds,
                    source_bounds,
                );
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

    fn composite_surface_src_over_with_stack(
        &mut self,
        target: CubeRenderTarget,
        source: &CubeBuffer<u32>,
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) {
        match target {
            CubeRenderTarget::Main => FilterPipeline::composite_src_over_surface_stack_region(
                &self.client,
                &self.scene,
                &self.scan,
                &mut self.target,
                source,
                self.size,
                source_size,
                source_origin,
                bounds,
                layer_stack.start as u32,
                layer_stack.end as u32,
                self.max_group_depth,
            ),
            CubeRenderTarget::Scratch(target_ix) => {
                FilterPipeline::composite_src_over_surface_stack_region(
                    &self.client,
                    &self.scene,
                    &self.scan,
                    &mut self.scratch[target_ix],
                    source,
                    self.size,
                    source_size,
                    source_origin,
                    bounds,
                    layer_stack.start as u32,
                    layer_stack.end as u32,
                    self.max_group_depth,
                )
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

fn encode_mask_kind(kind: MaskKind) -> u32 {
    match kind {
        MaskKind::Alpha => SVG_MASK_ALPHA,
        MaskKind::Luminance => SVG_MASK_LUMINANCE,
    }
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

fn encode_turbulence_kind(kind: TurbulenceKind) -> u32 {
    match kind {
        TurbulenceKind::Turbulence => 0,
        TurbulenceKind::FractalNoise => 1,
    }
}

fn encode_color_channel(channel: ColorChannel) -> u32 {
    match channel {
        ColorChannel::R => 0,
        ColorChannel::G => 1,
        ColorChannel::B => 2,
        ColorChannel::A => 3,
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
