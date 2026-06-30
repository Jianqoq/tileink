use crate::shared::{
    execution::{ExecOp, ExecPlan, LayerStackEntry},
    layer::{
        Layer,
        filter::{Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind},
    },
};

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
                Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) | Layer::ClipSdf { .. } => {
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

pub(super) fn filter_scratch_extra(filter: &Filter) -> usize {
    match filter {
        Filter::Chain { filters, .. } => {
            filters.iter().map(filter_scratch_extra).max().unwrap_or(0)
        }
        Filter::Graph { primitives, .. } => graph_scratch_extra(primitives),
        Filter::RectLiquidGlass(glass) => 2 + usize::from(glass.blur_radius > 0),
        Filter::Blur {
            std_dev_x,
            std_dev_y,
        } => usize::from(std_dev_x.max(*std_dev_y) > 0.0),
        Filter::ConvolveMatrix(_) => 1,
        Filter::DiffuseLighting(_) => 1,
        Filter::SpecularLighting(_) => 1,
        Filter::Offset { .. } => 1,
        Filter::Morphology { .. } => 2,
        Filter::DropShadow { std_dev, .. } => 1 + usize::from(std_dev.max(0.0) > 0.0),
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

#[cfg(test)]
mod tests {
    use peniko::{Color, kurbo::Rect};

    use crate::{FillRule, Radius, Scene, shared::execution::ROOT_COMMAND_LIST_ID};

    use super::required_scratch_count;

    #[test]
    fn sdf_clip_layer_requires_source_and_mask_scratch() {
        let mut scene = Scene::new(32, 32);
        scene.push_clip_sdf_rect_layer(Rect::new(4.0, 4.0, 28.0, 28.0), Radius::all(4.0));
        scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 32.0),
            Radius::ZERO,
            Color::WHITE,
            FillRule::NonZero,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);

        assert_eq!(required_scratch_count(&plan), 2);
    }
}
