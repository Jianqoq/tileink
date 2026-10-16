//! Shared painter ordering and live-batch accounting for in-place draw targets.

use super::incremental::IncrementalRenderStats;
use super::output::RenderTargetId;
use crate::Canvas;
use crate::shared::execution::ExecOp;
use std::ops::Range;

pub(crate) struct DrawBatch<'a> {
    pub(crate) draws: &'a [usize],
    pub(crate) id: u32,
    pub(crate) layers: Range<usize>,
}

impl<'a> DrawBatch<'a> {
    fn from_op(op: &'a ExecOp) -> Self {
        let ExecOp::DrawBatch {
            draws,
            batch_id,
            layer_stack,
            ..
        } = op
        else {
            unreachable!("direct root index points to a draw batch")
        };
        Self {
            draws,
            id: *batch_id,
            layers: layer_stack.clone(),
        }
    }
}

/// Tileink batch operations, statically dispatched for each renderer instance.
/// An error stops the plan; the owning command batch retains submitted leases
/// and decides whether any remaining unsubmitted commands can be committed.
pub(crate) trait DrawBatchAdapter {
    type Error;
    fn stats_mut(&mut self) -> &mut IncrementalRenderStats;
    /// Record the complete draw before returning; transient coarse results belong
    /// to this call and cannot leak into another target or nested scene.
    fn draw(
        &mut self,
        batches: Range<u32>,
        layers: Range<u32>,
        target: RenderTargetId,
    ) -> Result<(), Self::Error>;
}

pub(crate) fn execute_draw_batch<A: DrawBatchAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    batch: &DrawBatch<'_>,
    target: RenderTargetId,
) -> Result<(), A::Error> {
    if !draw_batch_is_live(canvas, batch.draws, batch.id) {
        return Ok(());
    }
    let stats = adapter.stats_mut();
    stats.draw_batches = stats.draw_batches.saturating_add(1);
    if target == RenderTargetId::Main {
        stats.root_draw_batches = stats.root_draw_batches.saturating_add(1);
    }
    adapter.draw(
        batch.id..batch.id.saturating_add(1),
        batch.layers.start as u32..batch.layers.end as u32,
        target,
    )
}

/// Execute root batches in painter order, stopping at the first recording error.
pub(crate) fn execute_in_place_root_batches<A: DrawBatchAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    ops: &[ExecOp],
    indices: &[usize],
) -> Result<(), A::Error> {
    for &index in indices {
        execute_draw_batch(
            adapter,
            canvas,
            &DrawBatch::from_op(&ops[index]),
            RenderTargetId::Main,
        )?;
    }
    Ok(())
}

fn draw_batch_is_live(scene: &Canvas, draws: &[usize], batch_id: u32) -> bool {
    scene
        .retained_painter()
        .map_or(!draws.is_empty(), |painter| {
            painter
                .batch_counts
                .get(batch_id as usize)
                .copied()
                .unwrap_or(0)
                != 0
        })
}

#[cfg(test)]
mod tests;
