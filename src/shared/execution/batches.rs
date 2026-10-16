//! Batch membership, painter ordering and root dispatch indexes.
use super::*;

impl ExecPlan {
    /// Removes artificial GPU batch boundaries introduced by materialized retained scenes.
    ///
    /// Retained scenes carry CPU-side identity and offscreen ownership, but adjacent plain draws
    /// with the same effective layer stack have exactly the same raster semantics as one batch.
    /// Coalescing them is important for component trees: otherwise every cached drawable would
    /// emit another coarse/fine dispatch pair even during a full redraw.
    pub(crate) fn coalesce_draw_batches(&mut self) {
        coalesce_draw_batches_in(&mut self.ops, &self.layer_stack_data);
        compact_layer_stacks(&mut self.ops, &mut self.layer_stack_data);
    }

    pub(crate) fn finalize_draw_batches(&mut self, draw_capacity: usize) {
        Rc::make_mut(&mut self.draw_order).clear();
        Rc::make_mut(&mut self.draw_batch_ids).clear();
        Rc::make_mut(&mut self.draw_batch_ids).resize(draw_capacity, u32::MAX);
        let mut next_batch = 0;
        finalize_draw_batches_in(
            &mut self.ops,
            Rc::make_mut(&mut self.draw_order),
            Rc::make_mut(&mut self.draw_batch_ids).as_mut_slice(),
            &mut next_batch,
        );
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
    }

    pub(crate) fn insert_plain_batch(&mut self, index: usize, draws: Vec<usize>) -> Option<u32> {
        if draws.is_empty() {
            return None;
        }
        assert!(index <= self.ops.len());
        let batch_id = max_batch_id(&self.ops).map_or(0, |batch| batch.saturating_add(1));
        self.ops.insert(
            index,
            ExecOp::DrawBatch {
                draws: Rc::new(draws),
                batch_id,
                owners: Rc::new(Vec::new()),
                layer_stack: 0..0,
            },
        );
        self.refresh_direct_root_batch_ops();
        Some(batch_id)
    }

    pub(super) fn refresh_retained_batch_ids(&mut self) {
        let mut candidates = HashMap::new();
        collect_retained_batch_ids(&self.ops, &mut candidates);
        self.retained_batch_ids = candidates
            .into_iter()
            .filter_map(|(owner, batch)| batch.map(|batch| (owner, batch)))
            .collect();
    }

    pub(super) fn refresh_direct_root_batch_ops(&mut self) {
        if self.ops.iter().any(|op| {
            matches!(
                op,
                ExecOp::OffscreenLayer { .. } | ExecOp::OffscreenMaskLayer { .. }
            )
        }) {
            self.direct_root_batch_ops = None;
            return;
        }
        self.direct_root_batch_ops = Some(
            self.ops
                .iter()
                .enumerate()
                .filter_map(|(index, op)| match op {
                    ExecOp::DrawBatch { batch_id, .. } => Some((*batch_id, index)),
                    _ => None,
                })
                .collect(),
        );
    }

    pub(crate) fn active_direct_root_ops(&self, active_batches: &[u32]) -> Option<Vec<usize>> {
        let locations = self.direct_root_batch_ops.as_ref()?;
        let mut ops = active_batches
            .iter()
            .filter_map(|batch| locations.get(batch).copied())
            .collect::<Vec<_>>();
        ops.sort_unstable();
        Some(ops)
    }

    pub(crate) fn all_direct_root_ops(&self) -> Option<Vec<usize>> {
        let mut ops = self
            .direct_root_batch_ops
            .as_ref()?
            .values()
            .copied()
            .collect::<Vec<_>>();
        ops.sort_unstable();
        Some(ops)
    }

    pub(super) fn refresh_draw_metadata(&mut self) {
        let mut order = Vec::new();
        let mut batches = vec![u32::MAX; self.draw_batch_ids.len()];
        collect_draw_metadata(&self.ops, &mut order, &mut batches);
        self.draw_order = Rc::new(order);
        self.draw_batch_ids = Rc::new(batches);
    }

    /// Disables a batch whose complete membership moved into a later persistent fragment.
    ///
    /// Partial migration deliberately leaves the shared draw list untouched: GPU-side stable
    /// batch IDs filter those members without cloning a potentially scene-sized `Rc<Vec<_>>`.
    pub(crate) fn remove_batch_if_all_moved(
        &mut self,
        batch_id: u32,
        moved_draws: usize,
    ) -> Option<(usize, ExecOp)> {
        let index = self.ops.iter().position(|op| {
            matches!(op, ExecOp::DrawBatch { batch_id: candidate, .. } if *candidate == batch_id)
        })?;
        let ExecOp::DrawBatch { draws, .. } = &self.ops[index] else {
            unreachable!("matched draw batch")
        };
        if draws.len() != moved_draws {
            return None;
        }
        let op = self.ops.remove(index);
        self.refresh_direct_root_batch_ops();
        Some((index, op))
    }

    pub(crate) fn restore_removed_batch(&mut self, index: usize, op: ExecOp) {
        self.ops.insert(index, op);
        self.refresh_direct_root_batch_ops();
    }
}

fn collect_draw_metadata(ops: &[ExecOp], order: &mut Vec<u32>, batches: &mut Vec<u32>) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                draws, batch_id, ..
            } => {
                for &draw in draws.iter() {
                    if batches.len() <= draw {
                        batches.resize(draw + 1, u32::MAX);
                    }
                    order.push(draw as u32);
                    batches[draw] = *batch_id;
                }
            }
            ExecOp::OffscreenLayer { children, .. } => {
                collect_draw_metadata(children, order, batches);
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_draw_metadata(content, order, batches);
                collect_draw_metadata(mask, order, batches);
            }
        }
    }
}

fn collect_retained_batch_ids(ops: &[ExecOp], out: &mut HashMap<RetainedBatchOwner, Option<u32>>) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                batch_id, owners, ..
            } => {
                for &owner in owners.iter() {
                    out.entry(owner)
                        .and_modify(|existing| {
                            if *existing != Some(*batch_id) {
                                *existing = None;
                            }
                        })
                        .or_insert(Some(*batch_id));
                }
            }
            ExecOp::OffscreenLayer { children, .. } => collect_retained_batch_ids(children, out),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_retained_batch_ids(content, out);
                collect_retained_batch_ids(mask, out);
            }
        }
    }
}

pub(super) fn max_batch_id(ops: &[ExecOp]) -> Option<u32> {
    ops.iter().fold(None, |maximum, op| {
        let candidate = match op {
            ExecOp::DrawBatch { batch_id, .. } => Some(*batch_id),
            ExecOp::OffscreenLayer { children, .. } => max_batch_id(children),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                max_batch_id(content).max(max_batch_id(mask))
            }
        };
        maximum.max(candidate)
    })
}

fn finalize_draw_batches_in(
    ops: &mut [ExecOp],
    draw_order: &mut Vec<u32>,
    draw_batch_ids: &mut [u32],
    next_batch: &mut u32,
) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                draws, batch_id, ..
            } => {
                *batch_id = *next_batch;
                *next_batch = next_batch.saturating_add(1);
                for &draw in draws.iter() {
                    draw_order.push(draw as u32);
                    draw_batch_ids[draw] = *batch_id;
                }
            }
            ExecOp::OffscreenLayer { children, .. } => {
                finalize_draw_batches_in(children, draw_order, draw_batch_ids, next_batch);
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                finalize_draw_batches_in(content, draw_order, draw_batch_ids, next_batch);
                finalize_draw_batches_in(mask, draw_order, draw_batch_ids, next_batch);
            }
        }
    }
}

fn coalesce_draw_batches_in(ops: &mut Vec<ExecOp>, layer_stacks: &[LayerStackEntry]) {
    for op in ops.iter_mut() {
        match op {
            ExecOp::OffscreenLayer { children, .. } => {
                coalesce_draw_batches_in(children, layer_stacks);
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                coalesce_draw_batches_in(content, layer_stacks);
                coalesce_draw_batches_in(mask, layer_stacks);
            }
            ExecOp::DrawBatch { .. } => {}
        }
    }

    let mut coalesced = Vec::with_capacity(ops.len());
    for op in ops.drain(..) {
        if let ExecOp::DrawBatch {
            draws,
            layer_stack,
            owners,
            ..
        } = &op
            && let Some(ExecOp::DrawBatch {
                draws: previous_draws,
                layer_stack: previous_stack,
                owners: previous_owners,
                ..
            }) = coalesced.last_mut()
            && layer_stacks[previous_stack.clone()] == layer_stacks[layer_stack.clone()]
        {
            Rc::make_mut(previous_draws).extend_from_slice(draws);
            for &owner in owners.iter() {
                if !previous_owners.contains(&owner) {
                    Rc::make_mut(previous_owners).push(owner);
                }
            }
            continue;
        }
        coalesced.push(op);
    }
    *ops = coalesced;
}
