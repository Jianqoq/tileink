//! Stable batch and stack relocation for retained subtree edits.
use super::*;

impl ExecPlan {
    /// Inserts an independently compiled root fragment without rebuilding unrelated plan ops.
    /// Layer-stack storage remains append-only, while the small root op sequence preserves scene
    /// painter order for middle insertion and reparenting.
    pub(crate) fn insert_fragment(
        &mut self,
        index: usize,
        mut fragment: ExecPlan,
    ) -> (Range<usize>, Vec<(usize, u32)>) {
        assert!(index <= self.ops.len());
        let stack_base = self.layer_stack_data.len();
        offset_layer_stack_ranges(&mut fragment.ops, stack_base);
        self.layer_stack_data.extend(fragment.layer_stack_data);
        let mut next_batch = max_batch_id(&self.ops).map_or(0, |batch| batch.saturating_add(1));
        remap_fragment_batches(&mut fragment.ops, &mut next_batch);
        let mut draw_batches = Vec::new();
        collect_fragment_draw_batches(&fragment.ops, &mut draw_batches);
        let len = fragment.ops.len();
        self.ops.splice(index..index, fragment.ops);
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
        (index..index + len, draw_batches)
    }

    pub(crate) fn remove_fragment(&mut self, range: Range<usize>) {
        self.ops.drain(range);
        compact_layer_stacks(&mut self.ops, &mut self.layer_stack_data);
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
    }

    /// Replaces one retained offscreen subtree while preserving stable batches owned by
    /// unchanged branches. Layer-stack records are append-only here; arena compaction can reclaim
    /// superseded records without shifting offsets during an ordinary transaction.
    pub(crate) fn replace_retained_offscreen_fragment(
        &mut self,
        id: RetainedNodeId,
        mut fragment: ExecPlan,
    ) -> Option<Vec<(usize, u32)>> {
        if fragment.ops.len() != 1 || find_retained_offscreen(&fragment.ops, id).is_none() {
            return None;
        }
        let stack_base = self.layer_stack_data.len();
        offset_layer_stack_ranges(&mut fragment.ops, stack_base);
        let mut replacement = fragment.ops.pop().unwrap();
        let mut next_batch = max_batch_id(&self.ops).map_or(0, |batch| batch.saturating_add(1));
        let mut used = std::collections::HashSet::new();
        let mut draw_batches = Vec::new();
        remap_fragment_batches_reusing(
            std::slice::from_mut(&mut replacement),
            &self.retained_batch_ids,
            &mut used,
            &mut next_batch,
            &mut draw_batches,
        );
        if !replace_retained_offscreen(&mut self.ops, id, replacement) {
            return None;
        }
        self.layer_stack_data.extend(fragment.layer_stack_data);
        self.refresh_draw_metadata();
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
        Some(draw_batches)
    }
}

fn collect_fragment_draw_batches(ops: &[ExecOp], out: &mut Vec<(usize, u32)>) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                draws, batch_id, ..
            } => out.extend(draws.iter().map(|&draw| (draw, *batch_id))),
            ExecOp::OffscreenLayer { children, .. } => collect_fragment_draw_batches(children, out),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_fragment_draw_batches(content, out);
                collect_fragment_draw_batches(mask, out);
            }
        }
    }
}

fn remap_fragment_batches_reusing(
    ops: &mut [ExecOp],
    retained_batches: &HashMap<RetainedBatchOwner, u32>,
    used: &mut std::collections::HashSet<u32>,
    next: &mut u32,
    draw_batches: &mut Vec<(usize, u32)>,
) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                draws,
                batch_id,
                owners,
                ..
            } => {
                let reusable = owners
                    .iter()
                    .filter_map(|owner| retained_batches.get(owner).copied())
                    .find(|batch| !used.contains(batch));
                *batch_id = reusable.unwrap_or_else(|| {
                    let batch = *next;
                    *next = next.saturating_add(1);
                    batch
                });
                used.insert(*batch_id);
                draw_batches.extend(draws.iter().map(|&draw| (draw, *batch_id)));
            }
            ExecOp::OffscreenLayer { children, .. } => {
                remap_fragment_batches_reusing(children, retained_batches, used, next, draw_batches)
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                remap_fragment_batches_reusing(content, retained_batches, used, next, draw_batches);
                remap_fragment_batches_reusing(mask, retained_batches, used, next, draw_batches);
            }
        }
    }
}

fn remap_fragment_batches(ops: &mut [ExecOp], next: &mut u32) {
    for op in ops {
        match op {
            ExecOp::DrawBatch { batch_id, .. } => {
                *batch_id = *next;
                *next = next.saturating_add(1);
            }
            ExecOp::OffscreenLayer { children, .. } => remap_fragment_batches(children, next),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                remap_fragment_batches(content, next);
                remap_fragment_batches(mask, next);
            }
        }
    }
}
