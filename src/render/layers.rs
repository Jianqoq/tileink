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
    shared::{execution::ExecPlan, layer::Layer},
};

#[derive(Debug, PartialEq)]
pub(crate) enum LayerError<E> {
    UnexpectedClip,
    Adapter(E),
}

/// Keep layer semantics independent of the selected API. A fused clip is not a
/// valid offscreen operation; rejecting it must not draw children or alter cursors.
pub(crate) fn execute<A: GroupAdapter + FilterAdapter + BackdropAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    op: Offscreen<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), LayerError<A::Error>> {
    let (opacity, blend) = match op.layer {
        Layer::Isolate => (None, None),
        Layer::Opacity(layer) => (Some(layer.opacity), None),
        Layer::Blend(layer) => (None, Some(layer.mode)),
        Layer::Filter {
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
            )
            .map_err(LayerError::Adapter);
        }
        Layer::Backdrop {
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
            )
            .map_err(LayerError::Adapter);
        }
        Layer::Clip | Layer::ClipSdf { .. } => return Err(LayerError::UnexpectedClip),
    };
    groups::execute(
        adapter,
        canvas,
        plan,
        Group {
            retained_id: op.retained_id,
            draw: op.draw,
            outer_stack: op.outer_stack,
            children: op.children,
            opacity,
            blend,
        },
        target,
        cursors,
    )
    .map_err(LayerError::Adapter)
}

#[cfg(test)]
mod tests;
