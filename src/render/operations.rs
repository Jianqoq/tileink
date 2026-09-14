//! Shared painter-order traversal of the prepared execution plan.
//!
//! Fused stack markers are already represented in draw-batch layer metadata.
//! Offscreen operations retain their own cache, damage and resource-cursor policy;
//! a root active-batch list must not suppress those effects or filter descendants.

use super::{
    draw_batches::{DrawBatch, DrawBatchAdapter, execute_draw_batch},
    filter_resources::cursors::FilterCursors,
    output::RenderTargetId,
};
use crate::canvas::{Canvas, RetainedSurfaceId};
use crate::shared::{
    execution::{ExecOp, ExecPlan},
    layer::{Layer, mask::Mask},
};
use std::ops::Range;

pub(crate) struct Offscreen<'a> {
    pub(crate) retained_id: Option<RetainedSurfaceId>,
    pub(crate) draw: usize,
    pub(crate) layer: &'a Layer,
    pub(crate) outer_stack: Range<usize>,
    pub(crate) children: &'a [ExecOp],
}

pub(crate) struct Masked<'a> {
    pub(crate) retained_id: Option<RetainedSurfaceId>,
    pub(crate) layer: &'a Mask,
    pub(crate) outer_stack: Range<usize>,
    pub(crate) content: &'a [ExecOp],
    pub(crate) mask: &'a [ExecOp],
}

pub(crate) trait OperationAdapter: DrawBatchAdapter {
    fn offscreen(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Offscreen<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error>;
    fn mask(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Masked<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error>;
}

/// `active_batches` is the sorted root batch selection from scene damage.
/// Layer adapters re-enter this traversal with their own local damage decision.
/// The first error reaches the frame owner without visiting any later operation.
pub(crate) fn execute_ops<A: OperationAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    ops: &[ExecOp],
    target: RenderTargetId,
    cursors: &mut FilterCursors,
    active_batches: Option<&[u32]>,
) -> Result<(), A::Error> {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                draws,
                batch_id,
                layer_stack,
                ..
            } => {
                if active_batches.is_some_and(|active| active.binary_search(batch_id).is_err()) {
                    continue;
                }
                execute_draw_batch(
                    adapter,
                    canvas,
                    DrawBatch {
                        draws,
                        id: *batch_id,
                        layers: layer_stack.clone(),
                    },
                    target,
                )?;
            }
            ExecOp::BeginClip
            | ExecOp::EndClip
            | ExecOp::BeginOpacity
            | ExecOp::EndOpacity
            | ExecOp::BeginBlend
            | ExecOp::EndBlend => {}
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                outer_stack,
                children,
            } => {
                adapter.offscreen(
                    canvas,
                    plan,
                    Offscreen {
                        retained_id: *retained_id,
                        draw: *draw,
                        layer,
                        outer_stack: outer_stack.clone(),
                        children,
                    },
                    target,
                    cursors,
                )?;
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                outer_stack,
                content,
                mask,
            } => {
                adapter.mask(
                    canvas,
                    plan,
                    Masked {
                        retained_id: *retained_id,
                        layer,
                        outer_stack: outer_stack.clone(),
                        content,
                        mask,
                    },
                    target,
                    cursors,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
