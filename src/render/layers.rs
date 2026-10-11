//! Shared meaning and execution routing of offscreen layer operations.

use super::{
    backdrops::{self, BackdropAdapter, BackdropLayer},
    filter_resources::cursors::FilterCursors,
    filters::{self, FilterAdapter, FilterLayer},
    groups::{self, Group, GroupAdapter},
    operations::Offscreen,
    output::RenderTargetId,
};
use crate::{
    Canvas,
    shared::execution::{ExecPlan, OffscreenLayer},
};
/// Execute the compiler's surface-owning operation, preserving adapter errors.
pub(crate) fn execute<A: GroupAdapter + FilterAdapter + BackdropAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    op: Offscreen<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    let (draw, opacity, blend) = match op.layer {
        OffscreenLayer::Isolate { draw } => (*draw, None, None),
        OffscreenLayer::Opacity { draw, opacity } => (*draw, Some(*opacity), None),
        OffscreenLayer::Blend { draw, mode } => (*draw, None, Some(*mode)),
        OffscreenLayer::Filter {
            filter,
            sample_region,
        } => {
            return filters::execute(
                adapter,
                canvas,
                plan,
                FilterLayer {
                    retained_id: op.retained_id,
                    filter,
                    region: sample_region,
                    stack: op.outer_stack,
                    children: op.children,
                },
                target,
                cursors,
            );
        }
        OffscreenLayer::Backdrop {
            filter,
            sample_region,
        } => {
            return backdrops::execute(
                adapter,
                canvas,
                plan,
                BackdropLayer {
                    retained_id: op.retained_id,
                    filter,
                    region: sample_region,
                    stack: op.outer_stack,
                    children: op.children,
                },
                target,
                cursors,
            );
        }
    };
    groups::execute(
        adapter,
        canvas,
        plan,
        Group {
            retained_id: op.retained_id,
            draw,
            outer_stack: op.outer_stack,
            children: op.children,
            opacity,
            blend,
        },
        target,
        cursors,
    )
}

#[cfg(test)]
mod tests;
