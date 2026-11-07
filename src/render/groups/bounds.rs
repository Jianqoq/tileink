use crate::{
    Canvas,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan, LayerStackEntry, OffscreenLayer},
        layer::filter,
    },
};

/// Opacity is not a clip. Derive its finite work region from descendant paint,
/// including nested effect output, instead of a caller's layout rectangle.
pub(super) fn content_bounds(canvas: &Canvas, plan: &ExecPlan, ops: &[ExecOp]) -> Bounds {
    ops.iter().fold(Bounds::new(0, 0, 0, 0), |bounds, op| {
        let (output, stack) = match op {
            ExecOp::DrawBatch {
                draws, layer_stack, ..
            } => (
                draws.iter().fold(Bounds::new(0, 0, 0, 0), |bounds, &draw| {
                    bounds.union(super::draw_bounds(canvas, draw))
                }),
                layer_stack,
            ),
            ExecOp::OffscreenLayer {
                layer,
                children,
                outer_stack,
                ..
            } => (
                match layer {
                    OffscreenLayer::Isolate { draw }
                    | OffscreenLayer::Opacity {
                        draw: Some(draw), ..
                    }
                    | OffscreenLayer::Blend { draw, .. } => super::draw_bounds(canvas, *draw),
                    OffscreenLayer::Opacity { draw: None, .. } => {
                        content_bounds(canvas, plan, children)
                    }
                    OffscreenLayer::Filter {
                        filter: value,
                        sample_region,
                    } => filter::unclipped_filtered_region_bounds(value, sample_region),
                    OffscreenLayer::Backdrop {
                        filter: value,
                        sample_region,
                        ..
                    } => filter::unclipped_filtered_region_bounds(value, sample_region)
                        .union(content_bounds(canvas, plan, children)),
                },
                outer_stack,
            ),
            ExecOp::OffscreenMaskLayer {
                layer, outer_stack, ..
            } => (filter::region_bounds(&layer.region), outer_stack),
        };
        let output = plan.layer_stack_data[stack.clone()]
            .iter()
            .fold(output, |bounds, entry| {
                let draw = match entry {
                    LayerStackEntry::Clip { draw }
                    | LayerStackEntry::Opacity { draw, .. }
                    | LayerStackEntry::Blend { draw, .. } => *draw,
                };
                bounds.intersect(super::draw_bounds(canvas, draw as usize))
            });
        bounds.union(output)
    })
}
