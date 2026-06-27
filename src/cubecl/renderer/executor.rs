use ::cubecl::prelude::Runtime;
use peniko::{BlendMode, kurbo::Shape};

use crate::{
    scene::Scene,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        layer::{Layer, filter::Filter, region::Region},
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
        },
        fine::FinePipeline,
    },
};

use super::{CubeRenderTarget, Renderer};

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
                next_filter_path_index(sample_region, filter_path_cursor);
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

pub(super) fn required_scratch_count(plan: &ExecPlan) -> usize {
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
