//! Shared root-work counting and one-shot early-submission policy.

use crate::Canvas;
use crate::shared::{execution::ExecOp, layer::Layer};

#[derive(Default)]
pub(crate) struct BatchSchedule {
    submissions: u32,
    root_batches_before_submit: Option<usize>,
}

impl BatchSchedule {
    pub(crate) fn set_initial_root_batch_budget(&mut self, batches: usize) {
        self.root_batches_before_submit = Some(batches);
    }

    /// Request submission only before a real successor root batch. A uniform
    /// rollover already starts the GPU, so it cancels the latency submission.
    pub(crate) fn begin_root_batch(&mut self) -> bool {
        let Some(remaining) = self.root_batches_before_submit.take() else {
            return false;
        };
        if self.submissions == 0 {
            if remaining == 0 {
                return true;
            }
            self.root_batches_before_submit = Some(remaining - 1);
        }
        false
    }

    /// Count a successful, nonempty adapter submission, never merely a request.
    pub(crate) fn record_submission(&mut self) {
        self.submissions = self
            .submissions
            .checked_add(1)
            .expect("frame submission count overflow");
    }

    pub(crate) fn submissions(&self) -> u32 {
        self.submissions
    }
}

pub(crate) fn draw_batch_is_live(scene: &Canvas, draws: &[usize], batch_id: u32) -> bool {
    scene
        .stable_batch_counts
        .as_ref()
        .map_or(!draws.is_empty(), |counts| {
            counts.get(batch_id as usize).copied().unwrap_or(0) != 0
        })
}

// Backdrop foreground always executes on its parent's target, even when the
// effect's bounds are empty. Other offscreen layers execute children in scratch.
// Count the actual Main work to preserve eligibility and the first-quarter budget.
pub(crate) fn root_draw_batch_count(canvas: &Canvas, ops: &[ExecOp]) -> usize {
    ops.iter()
        .map(|op| match op {
            ExecOp::DrawBatch {
                draws, batch_id, ..
            } => usize::from(draw_batch_is_live(canvas, draws, *batch_id)),
            ExecOp::OffscreenLayer {
                layer: Layer::Backdrop { .. },
                children,
                ..
            } => root_draw_batch_count(canvas, children),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests;

/// Preserve the existing first-quarter overlap policy in one shared algorithm.
/// Small, incremental and ping-pong frames keep their single-submission path.
pub(crate) fn initial_root_batch_budget(
    canvas: &Canvas,
    ops: &[ExecOp],
    size: (u32, u32),
    allow_early_submit: bool,
    partial: bool,
    portable: bool,
) -> Option<usize> {
    if !allow_early_submit
        || partial
        || portable
        || u64::from(size.0) * u64::from(size.1) < 1024 * 1024
    {
        return None;
    }
    let count = root_draw_batch_count(canvas, ops);
    (count >= 16).then_some(count / 4)
}
