//! Frame ordering shared by all GPU adapters. Resources and encoding stay in the adapter.

use super::{
    batches::initial_root_batch_budget, damage_tiles::DamageTiles, profile::cpu::profile_cpu,
};
use crate::{Canvas, shared::execution::ExecPlan};
use std::rc::Rc;

/// The adapter owns device readiness, physical allocations and actual GPU operations.
/// Methods returning an error must leave submitted resources owned by their retirement path.
pub(crate) trait FrameAdapter {
    type Error;
    fn recycle_previous_frame(&mut self);
    fn active_tiles(&self) -> Option<&DamageTiles>;
    fn size(&self) -> (u32, u32);
    fn prepared_plan(&self) -> Option<Rc<ExecPlan>>;
    fn portable_textures(&self) -> bool;
    fn prepare_frame_resources(&mut self) -> Result<(), Self::Error>;
    fn encode_vector_images(&mut self) -> Result<(), Self::Error>;
    fn set_initial_root_batch_budget(&mut self, budget: usize);
    fn scan_scene(&mut self, canvas: &Canvas) -> Result<(), Self::Error>;
    fn clear_root(&mut self, partial: bool) -> Result<(), Self::Error>;
    /// Called only for a partial frame; selection may update cached spatial queries.
    fn active_batch_ids(&mut self, batch_ids: &[u32]) -> Vec<u32>;
    fn execute_direct_root(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> Result<(), Self::Error>;
    fn execute_recursive(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        active: Option<&[u32]>,
    ) -> Result<(), Self::Error>;
    fn copy_history_to_output(&mut self) -> Result<(), Self::Error>;
    fn record_filter_stats(&mut self);
}

#[derive(Debug, PartialEq)]
pub(crate) enum FrameError<E> {
    MissingPlan,
    Adapter(E),
}

/// Shared submission policy. Native immediate recordings are one ordered batch;
/// wgpu may submit a completed root prefix when its caller permits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubmissionPolicy {
    Single,
    EarlyRootBatches,
}

/// Preserve the empty-frame copy, stage order, first-quarter submission budget and
/// success-only history copy in one place. This is the production scheduler, not
/// an API-specific reimplementation or a second scene representation.
pub(crate) fn encode<A: FrameAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    copy_history: bool,
    submission_policy: SubmissionPolicy,
) -> Result<(), FrameError<A::Error>> {
    adapter.recycle_previous_frame();
    if adapter.active_tiles().is_some_and(DamageTiles::is_empty) {
        return if copy_history {
            adapter
                .copy_history_to_output()
                .map_err(FrameError::Adapter)
        } else {
            Ok(())
        };
    }
    let plan = adapter.prepared_plan().ok_or(FrameError::MissingPlan)?;
    adapter
        .prepare_frame_resources()
        .map_err(FrameError::Adapter)?;
    adapter
        .encode_vector_images()
        .map_err(FrameError::Adapter)?;
    let partial = adapter.active_tiles().is_some();
    if let Some(budget) = initial_root_batch_budget(
        canvas,
        &plan.ops,
        adapter.size(),
        submission_policy == SubmissionPolicy::EarlyRootBatches,
        partial,
        adapter.portable_textures(),
    ) {
        adapter.set_initial_root_batch_budget(budget);
    }
    adapter.scan_scene(canvas).map_err(FrameError::Adapter)?;
    adapter.clear_root(partial).map_err(FrameError::Adapter)?;
    let batch_ids = canvas
        .stable_batch_ids
        .as_deref()
        .unwrap_or(&plan.draw_batch_ids);
    let active = profile_cpu("plan.active_batches", || {
        partial.then(|| adapter.active_batch_ids(batch_ids))
    });
    let mut result = profile_cpu("plan.execute", || {
        let direct = active.as_ref().map_or_else(
            || plan.all_direct_root_ops(),
            |selected| plan.active_direct_root_ops(selected),
        );
        if let Some(ops) = direct {
            adapter.execute_direct_root(canvas, &plan, &ops)
        } else {
            adapter.execute_recursive(canvas, &plan, active.as_deref())
        }
    });
    if result.is_ok() && copy_history {
        result = adapter.copy_history_to_output();
    }
    adapter.record_filter_stats();
    result.map_err(FrameError::Adapter)
}

#[cfg(test)]
mod tests;
