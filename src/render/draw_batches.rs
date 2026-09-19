//! Shared draw ordering, live-batch accounting and root ping-pong execution.
//! Adapters own resources and encode coarse/fine/copies; they do not choose the
//! painter order or publish partially encoded portable work after an error.

use super::batches::draw_batch_is_live;
use super::incremental::IncrementalRenderStats;
use super::output::RenderTargetId;
use super::profile::cpu::profile_cpu;
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
    fn begin_root_batch(&mut self) -> Result<(), Self::Error>;
    fn coarse(&mut self, batches: Range<u32>, layers: Range<u32>) -> Result<(), Self::Error>;
    fn fine(&mut self, target: RenderTargetId) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy)]
pub(crate) enum RootBatchMode {
    Direct,
    Portable { partial: bool },
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum PingPongSide {
    Source,
    Destination,
}

impl PingPongSide {
    pub(crate) fn other(self) -> Self {
        match self {
            Self::Source => Self::Destination,
            Self::Destination => Self::Source,
        }
    }
}

/// These targets are full-sized root intermediates. Scratch draws retain their
/// separate local-copy/halo semantics and must not use this root-only protocol.
pub(crate) trait RootBatchAdapter: DrawBatchAdapter {
    fn prepare_portable_targets(&mut self) -> Result<(), Self::Error>;
    fn copy_root_to(&mut self, side: PingPongSide) -> Result<(), Self::Error>;
    fn copy_to_root(&mut self, side: PingPongSide) -> Result<(), Self::Error>;
    fn fine_portable(&mut self, source: PingPongSide) -> Result<(), Self::Error>;
}

fn prepare_draw_batch<A: DrawBatchAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    batch: DrawBatch<'_>,
    target: RenderTargetId,
) -> Result<bool, A::Error> {
    if !draw_batch_is_live(canvas, batch.draws, batch.id) {
        return Ok(false);
    }
    if target == RenderTargetId::Main {
        adapter.begin_root_batch()?;
    }
    let stats = adapter.stats_mut();
    stats.draw_batches = stats.draw_batches.saturating_add(1);
    if target == RenderTargetId::Main {
        stats.root_draw_batches = stats.root_draw_batches.saturating_add(1);
    }
    adapter.coarse(
        batch.id..batch.id.saturating_add(1),
        batch.layers.start as u32..batch.layers.end as u32,
    )?;
    Ok(true)
}

pub(crate) fn execute_draw_batch<A: DrawBatchAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    batch: DrawBatch<'_>,
    target: RenderTargetId,
) -> Result<(), A::Error> {
    profile_cpu("plan.draw_batch", || {
        if prepare_draw_batch(adapter, canvas, batch, target)? {
            adapter.fine(target)?;
        }
        Ok(())
    })
}

pub(crate) fn execute_direct_root_batches<A: RootBatchAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    ops: &[ExecOp],
    indices: &[usize],
    mode: RootBatchMode,
) -> Result<(), A::Error> {
    if indices.is_empty() {
        return Ok(());
    }
    let RootBatchMode::Portable { partial } = mode else {
        return execute_in_place_root_batches(adapter, canvas, ops, indices);
    };
    adapter.prepare_portable_targets()?;
    adapter.copy_root_to(PingPongSide::Source)?;
    adapter.stats_mut().portable_texture_copies += 1;
    if partial {
        // Sparse fine dispatch leaves the other tiles untouched on both sides.
        adapter.copy_root_to(PingPongSide::Destination)?;
        adapter.stats_mut().portable_texture_copies += 1;
    }
    let mut latest = PingPongSide::Source;
    let mut encoded_any = false;
    for &index in indices {
        let encoded = profile_cpu("plan.draw_batch", || {
            prepare_draw_batch(
                adapter,
                canvas,
                DrawBatch::from_op(&ops[index]),
                RenderTargetId::Main,
            )
        })?;
        if !encoded {
            continue;
        }
        adapter.fine_portable(latest)?;
        latest = latest.other();
        encoded_any = true;
    }
    if encoded_any {
        adapter.copy_to_root(latest)?;
        adapter.stats_mut().portable_texture_copies += 1;
    }
    Ok(())
}

/// In-place roots need only the common draw adapter, not portable ping-pong.
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
            DrawBatch::from_op(&ops[index]),
            RenderTargetId::Main,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
