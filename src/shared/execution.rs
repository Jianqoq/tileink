use std::{collections::HashMap, ops::Range, sync::Arc};

use peniko::BlendMode;

use crate::{
    NodeGeneration, PersistentLayerKey, RetainedNodeId,
    canvas::RetainedSurfaceId,
    shared::layer::{Layer, mask::Mask},
};

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RetainedBatchOwner {
    pub(crate) node: RetainedNodeId,
    pub(crate) branch: RetainedBatchBranch,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RetainedBatchBranch {
    Content,
    Mask,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LayerStackEntry {
    Clip { draw: u32 },
    Opacity { draw: u32, opacity: f32 },
    Blend { draw: u32, mode: BlendMode },
}

#[derive(Clone, Default)]
pub(crate) struct CommandList {
    pub commands: Vec<Command>,
}

#[derive(Clone)]
pub(crate) enum Command {
    Draw(usize),
    MaterializedRetainedScene {
        id: RetainedNodeId,
        revision: NodeGeneration,
        children: CommandListId,
    },
    Layer {
        retained: Option<PersistentLayerKey>,
        draw: usize,
        layer: Layer,
        children: CommandListId,
    },
    MaskLayer {
        retained: Option<PersistentLayerKey>,
        layer: Mask,
        content: CommandListId,
        mask: CommandListId,
    },
}

/// GPU-friendly linear execution plan for one command list.
///
/// `layer_stack_data` stores ordered fused layer stack snapshots referenced by
/// `ExecOp::DrawBatch`. The coarse stage replays this stack into tile-local
/// begin/end particles so fine owns clip, opacity, and blend semantics.
#[derive(Clone, Debug)]
pub(crate) struct ExecPlan {
    pub ops: Vec<ExecOp>,
    /// Batch-local fused layer stack snapshots in user nesting order.
    pub layer_stack_data: Vec<LayerStackEntry>,
    /// Physical draw slots in painter order. Retained arenas may keep stable slots that are not
    /// contiguous, so tile binning must not infer order from the draw-record buffer.
    pub draw_order: Arc<Vec<u32>>,
    /// Stable physical draw slot to coarse batch membership.
    pub draw_batch_ids: Arc<Vec<u32>>,
    /// Stable batch assigned to each retained layer branch, including empty branches.
    pub retained_batch_ids: HashMap<RetainedBatchOwner, u32>,
    /// Fused layer hidden draw to the stack entries that reference it. A descriptor update can
    /// patch its own entries without scanning every unrelated retained layer.
    pub(crate) layer_stack_locations: HashMap<u32, Vec<usize>>,
    /// Direct root batches are indexable when the plan contains only fused layers. Offscreen
    /// plans stay on the recursive executor because their control flow carries surface effects.
    pub(crate) direct_root_batch_ops: Option<HashMap<u32, usize>>,
}

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
        Arc::make_mut(&mut self.draw_order).clear();
        Arc::make_mut(&mut self.draw_batch_ids).clear();
        Arc::make_mut(&mut self.draw_batch_ids).resize(draw_capacity, u32::MAX);
        let mut next_batch = 0;
        finalize_draw_batches_in(
            &mut self.ops,
            Arc::make_mut(&mut self.draw_order),
            Arc::make_mut(&mut self.draw_batch_ids).as_mut_slice(),
            &mut next_batch,
        );
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
    }

    /// Patches one retained layer while preserving draw order and stable batch membership.
    /// Returns false when the replacement changes execution structure and must be recompiled.
    pub(crate) fn patch_retained_layer(
        &mut self,
        id: RetainedNodeId,
        old: &Command,
        new: &Command,
    ) -> bool {
        let (old_draw, new_draw, old_layer, new_layer) = match (old, new) {
            (
                Command::Layer {
                    draw: old_draw,
                    layer: old_layer,
                    ..
                },
                Command::Layer {
                    draw: new_draw,
                    layer: new_layer,
                    ..
                },
            ) if same_layer_plan_shape(old_layer, new_layer) => (
                Some(*old_draw),
                Some(*new_draw),
                Some(old_layer),
                Some(new_layer),
            ),
            (Command::MaskLayer { .. }, Command::MaskLayer { .. }) => (None, None, None, None),
            _ => return false,
        };

        if let (Some(old_draw), Some(new_draw), Some(new_layer)) = (old_draw, new_draw, new_layer) {
            let locations = self
                .layer_stack_locations
                .remove(&(old_draw as u32))
                .unwrap_or_default();
            for &index in &locations {
                let entry = &mut self.layer_stack_data[index];
                match (entry, new_layer) {
                    (LayerStackEntry::Clip { draw }, Layer::Clip | Layer::ClipSdf { .. })
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                    }
                    (LayerStackEntry::Opacity { draw, opacity }, Layer::Opacity(replacement))
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                        *opacity = replacement.opacity;
                    }
                    (LayerStackEntry::Blend { draw, mode }, Layer::Blend(replacement))
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                        *mode = replacement.mode;
                    }
                    _ => {}
                }
            }
            if matches!(
                new_layer,
                Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_)
            ) {
                self.layer_stack_locations
                    .entry(new_draw as u32)
                    .or_default()
                    .extend(locations);
                return true;
            }
        }
        patch_retained_layer_ops(&mut self.ops, id, old_draw, old_layer, new)
    }

    /// Updates only translated offscreen descriptors belonging to one retained scene leaf.
    ///
    /// Position changes preserve command topology, physical draw slots, and BatchIds. Replacing
    /// these descriptors avoids recompiling unrelated scene fragments while still moving filter,
    /// backdrop, group, and mask surface geometry to the new coordinates.
    pub(crate) fn patch_retained_scene_position(
        &mut self,
        id: RetainedNodeId,
        fragment: &ExecPlan,
    ) -> bool {
        let mut replacements = HashMap::new();
        collect_surface_patches(&fragment.ops, id, &mut replacements);
        if replacements.is_empty() {
            return false;
        }
        let mut patched = 0;
        patch_surface_ops(&mut self.ops, &replacements, &mut patched);
        patched == replacements.len()
    }

    pub(crate) fn layer_stack_ranges_for_draw(&self, draw: usize) -> Vec<Range<usize>> {
        let Some(locations) = self.layer_stack_locations.get(&(draw as u32)) else {
            return Vec::new();
        };
        let mut ranges = Vec::<Range<usize>>::new();
        for &index in locations {
            if let Some(last) = ranges.last_mut()
                && last.end == index
            {
                last.end += 1;
            } else {
                ranges.push(index..index + 1);
            }
        }
        ranges
    }

    /// Appends an already-compiled independent fragment and gives its batches fresh stable IDs.
    /// Draw-order arrays stay shared: persistent canvases provide painter keys and a separately
    /// patched physical-draw-to-batch table.
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

    /// Locates a separately compiled root subtree inside this plan. The retained content owner
    /// is present in both plans even when a fused layer itself has no retained-id-bearing op, so
    /// the relative owner position gives an exact contiguous fragment range.
    pub(crate) fn root_fragment_range(
        &self,
        fragment: &Self,
        id: RetainedNodeId,
        root_owner_ops: &HashMap<RetainedNodeId, usize>,
    ) -> Option<Range<usize>> {
        let owner = RetainedBatchOwner {
            node: id,
            branch: RetainedBatchBranch::Content,
        };
        let fragment_owner = root_owner_op_index(&fragment.ops, owner)?;
        let plan_owner = *root_owner_ops.get(&id)?;
        let start = plan_owner.checked_sub(fragment_owner)?;
        let end = start.checked_add(fragment.ops.len())?;
        (end <= self.ops.len()).then_some(start..end)
    }

    /// Builds the root owner index once when a full plan is materialized. Indexing every root
    /// fragment can then remain O(plan ops) total instead of rescanning the whole plan per layer.
    pub(crate) fn root_owner_op_indices(&self) -> HashMap<RetainedNodeId, usize> {
        let mut indices = HashMap::new();
        for (index, op) in self.ops.iter().enumerate() {
            match op {
                ExecOp::DrawBatch { owners, .. } => {
                    for owner in owners.iter() {
                        indices.entry(owner.node).or_insert(index);
                    }
                }
                ExecOp::OffscreenLayer { retained_id, .. }
                | ExecOp::OffscreenMaskLayer { retained_id, .. } => {
                    if let Some(retained) = retained_id {
                        indices.entry(retained.node).or_insert(index);
                    }
                }
                _ => {}
            }
        }
        indices
    }

    pub(crate) fn remove_fragment(&mut self, range: Range<usize>) -> bool {
        if range.start > range.end || range.end > self.ops.len() {
            return false;
        }
        self.ops.drain(range);
        compact_layer_stacks(&mut self.ops, &mut self.layer_stack_data);
        self.refresh_retained_batch_ids();
        self.refresh_layer_stack_locations();
        self.refresh_direct_root_batch_ops();
        true
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
                draws: Arc::new(draws),
                batch_id,
                owners: Arc::new(Vec::new()),
                layer_stack: 0..0,
            },
        );
        self.refresh_direct_root_batch_ops();
        Some(batch_id)
    }

    fn refresh_retained_batch_ids(&mut self) {
        let mut candidates = HashMap::new();
        collect_retained_batch_ids(&self.ops, &mut candidates);
        self.retained_batch_ids = candidates
            .into_iter()
            .filter_map(|(owner, batch)| batch.map(|batch| (owner, batch)))
            .collect();
    }

    fn refresh_layer_stack_locations(&mut self) {
        self.layer_stack_locations.clear();
        for (index, entry) in self.layer_stack_data.iter().enumerate() {
            let draw = match entry {
                LayerStackEntry::Clip { draw }
                | LayerStackEntry::Opacity { draw, .. }
                | LayerStackEntry::Blend { draw, .. } => *draw,
            };
            self.layer_stack_locations
                .entry(draw)
                .or_default()
                .push(index);
        }
    }

    fn refresh_direct_root_batch_ops(&mut self) {
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

    pub(crate) fn contains_retained_offscreen(&self, id: RetainedNodeId) -> bool {
        find_retained_offscreen(&self.ops, id)
    }

    pub(crate) fn retained_offscreen_ancestor_of(
        &self,
        owner: RetainedNodeId,
    ) -> Option<RetainedNodeId> {
        find_retained_offscreen_ancestor(&self.ops, owner, None)
    }

    pub(crate) fn reorder_root_offscreen(&mut self, order: &[RetainedNodeId]) -> bool {
        if self.ops.len() != order.len() {
            return false;
        }
        let mut by_node = HashMap::with_capacity(self.ops.len());
        for (index, op) in self.ops.iter().enumerate() {
            let node = match op {
                ExecOp::OffscreenLayer {
                    retained_id: Some(id),
                    ..
                }
                | ExecOp::OffscreenMaskLayer {
                    retained_id: Some(id),
                    ..
                } => id.node,
                _ => return false,
            };
            if by_node.insert(node, index).is_some() {
                return false;
            }
        }
        let Some(indices) = order
            .iter()
            .map(|id| by_node.get(id).copied())
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        self.ops = indices
            .into_iter()
            .map(|index| self.ops[index].clone())
            .collect();
        self.refresh_direct_root_batch_ops();
        true
    }

    /// Replaces one retained offscreen subtree while preserving stable batches owned by
    /// unchanged branches. Layer-stack records are append-only here; arena compaction can reclaim
    /// superseded records without shifting offsets during an ordinary transaction.
    pub(crate) fn replace_retained_offscreen_fragment(
        &mut self,
        id: RetainedNodeId,
        mut fragment: ExecPlan,
    ) -> Option<Vec<(usize, u32)>> {
        if fragment.ops.len() != 1 || !find_retained_offscreen(&fragment.ops, id) {
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

    fn refresh_draw_metadata(&mut self) {
        let mut order = Vec::new();
        let mut batches = vec![u32::MAX; self.draw_batch_ids.len()];
        collect_draw_metadata(&self.ops, &mut order, &mut batches);
        self.draw_order = Arc::new(order);
        self.draw_batch_ids = Arc::new(batches);
    }

    /// Disables a batch whose complete membership moved into a later persistent fragment.
    ///
    /// Partial migration deliberately leaves the shared draw list untouched: GPU-side stable
    /// batch IDs filter those members without cloning a potentially scene-sized `Arc<Vec<_>>`.
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

#[derive(Clone)]
enum SurfacePatch {
    Layer { draw: usize, layer: Layer },
    Mask { layer: Mask },
}

fn collect_surface_patches(
    ops: &[ExecOp],
    owner: RetainedNodeId,
    patches: &mut HashMap<RetainedSurfaceId, SurfacePatch>,
) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                children,
                ..
            } => {
                if let Some(id) = retained_id.filter(|id| id.node == owner) {
                    patches.insert(
                        id,
                        SurfacePatch::Layer {
                            draw: *draw,
                            layer: layer.clone(),
                        },
                    );
                }
                collect_surface_patches(children, owner, patches);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if let Some(id) = retained_id.filter(|id| id.node == owner) {
                    patches.insert(
                        id,
                        SurfacePatch::Mask {
                            layer: layer.clone(),
                        },
                    );
                }
                collect_surface_patches(content, owner, patches);
                collect_surface_patches(mask, owner, patches);
            }
            _ => {}
        }
    }
}

fn patch_surface_ops(
    ops: &mut [ExecOp],
    patches: &HashMap<RetainedSurfaceId, SurfacePatch>,
    patched: &mut usize,
) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                children,
                ..
            } => {
                if let Some(SurfacePatch::Layer {
                    draw: replacement_draw,
                    layer: replacement_layer,
                }) = retained_id.and_then(|id| patches.get(&id))
                {
                    *draw = *replacement_draw;
                    *layer = replacement_layer.clone();
                    *patched += 1;
                }
                patch_surface_ops(children, patches, patched);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if let Some(SurfacePatch::Mask { layer: replacement }) =
                    retained_id.and_then(|id| patches.get(&id))
                {
                    *layer = replacement.clone();
                    *patched += 1;
                }
                patch_surface_ops(content, patches, patched);
                patch_surface_ops(mask, patches, patched);
            }
            _ => {}
        }
    }
}

fn root_owner_op_index(ops: &[ExecOp], owner: RetainedBatchOwner) -> Option<usize> {
    ops.iter().position(|op| match op {
        ExecOp::DrawBatch { owners, .. } => owners.contains(&owner),
        ExecOp::OffscreenLayer { retained_id, .. }
        | ExecOp::OffscreenMaskLayer { retained_id, .. } => {
            retained_id.is_some_and(|retained| retained.node == owner.node)
        }
        _ => false,
    })
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
            _ => {}
        }
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
                collect_draw_metadata(children, order, batches)
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_draw_metadata(content, order, batches);
                collect_draw_metadata(mask, order, batches);
            }
            _ => {}
        }
    }
}

fn find_retained_offscreen(ops: &[ExecOp], id: RetainedNodeId) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::OffscreenLayer {
            retained_id,
            children,
            ..
        } => {
            retained_id.is_some_and(|retained| retained.node == id)
                || find_retained_offscreen(children, id)
        }
        ExecOp::OffscreenMaskLayer {
            retained_id,
            content,
            mask,
            ..
        } => {
            retained_id.is_some_and(|retained| retained.node == id)
                || find_retained_offscreen(content, id)
                || find_retained_offscreen(mask, id)
        }
        _ => false,
    })
}

fn find_retained_offscreen_ancestor(
    ops: &[ExecOp],
    owner: RetainedNodeId,
    ancestor: Option<RetainedNodeId>,
) -> Option<RetainedNodeId> {
    for op in ops {
        match op {
            ExecOp::DrawBatch { owners, .. }
                if owners.iter().any(|candidate| candidate.node == owner) =>
            {
                return ancestor;
            }
            ExecOp::OffscreenLayer {
                retained_id,
                children,
                ..
            } => {
                if retained_id.is_some_and(|candidate| candidate.node == owner) {
                    return ancestor;
                }
                if let Some(found) = find_retained_offscreen_ancestor(
                    children,
                    owner,
                    retained_id.map(|candidate| candidate.node).or(ancestor),
                ) {
                    return Some(found);
                }
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                content,
                mask,
                ..
            } => {
                if retained_id.is_some_and(|candidate| candidate.node == owner) {
                    return ancestor;
                }
                let current = retained_id.map(|candidate| candidate.node).or(ancestor);
                if let Some(found) = find_retained_offscreen_ancestor(content, owner, current)
                    .or_else(|| find_retained_offscreen_ancestor(mask, owner, current))
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn replace_retained_offscreen(ops: &mut [ExecOp], id: RetainedNodeId, replacement: ExecOp) -> bool {
    let mut replacement = Some(replacement);
    replace_retained_offscreen_inner(ops, id, &mut replacement)
}

fn replace_retained_offscreen_inner(
    ops: &mut [ExecOp],
    id: RetainedNodeId,
    replacement: &mut Option<ExecOp>,
) -> bool {
    for op in ops {
        let matches = match op {
            ExecOp::OffscreenLayer { retained_id, .. }
            | ExecOp::OffscreenMaskLayer { retained_id, .. } => {
                retained_id.is_some_and(|retained| retained.node == id)
            }
            _ => false,
        };
        if matches {
            *op = replacement.take().unwrap();
            return true;
        }
        let found = match op {
            ExecOp::OffscreenLayer { children, .. } => {
                replace_retained_offscreen_inner(children, id, replacement)
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                replace_retained_offscreen_inner(content, id, replacement)
                    || replace_retained_offscreen_inner(mask, id, replacement)
            }
            _ => false,
        };
        if found {
            return true;
        }
    }
    false
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
            _ => {}
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
            _ => {}
        }
    }
}

fn max_batch_id(ops: &[ExecOp]) -> Option<u32> {
    ops.iter().fold(None, |maximum, op| {
        let candidate = match op {
            ExecOp::DrawBatch { batch_id, .. } => Some(*batch_id),
            ExecOp::OffscreenLayer { children, .. } => max_batch_id(children),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                max_batch_id(content).max(max_batch_id(mask))
            }
            _ => None,
        };
        maximum.max(candidate)
    })
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
            _ => {}
        }
    }
}

fn offset_layer_stack_ranges(ops: &mut [ExecOp], offset: usize) {
    let offset_range = |range: &mut Range<usize>| {
        range.start += offset;
        range.end += offset;
    };
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => offset_range(layer_stack),
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                offset_range(outer_stack);
                offset_layer_stack_ranges(children, offset);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                offset_range(outer_stack);
                offset_layer_stack_ranges(content, offset);
                offset_layer_stack_ranges(mask, offset);
            }
            _ => {}
        }
    }
}

fn compact_layer_stacks(ops: &mut [ExecOp], layer_stacks: &mut Vec<LayerStackEntry>) {
    let old = std::mem::take(layer_stacks);
    remap_layer_stacks(ops, &old, layer_stacks);
}

fn remap_layer_stacks(
    ops: &mut [ExecOp],
    old: &[LayerStackEntry],
    compact: &mut Vec<LayerStackEntry>,
) {
    let remap = |range: &mut Range<usize>, compact: &mut Vec<LayerStackEntry>| {
        let stack = &old[range.clone()];
        if stack.is_empty() {
            *range = 0..0;
            return;
        }
        if compact.ends_with(stack) {
            *range = compact.len() - stack.len()..compact.len();
            return;
        }
        let start = compact.len();
        compact.extend_from_slice(stack);
        *range = start..compact.len();
    };
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => remap(layer_stack, compact),
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                remap(outer_stack, compact);
                remap_layer_stacks(children, old, compact);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                remap(outer_stack, compact);
                remap_layer_stacks(content, old, compact);
                remap_layer_stacks(mask, old, compact);
            }
            _ => {}
        }
    }
}

fn same_layer_plan_shape(old: &Layer, new: &Layer) -> bool {
    matches!(
        (old, new),
        (
            Layer::Clip | Layer::ClipSdf { .. },
            Layer::Clip | Layer::ClipSdf { .. }
        ) | (Layer::Isolate, Layer::Isolate)
            | (Layer::Opacity(_), Layer::Opacity(_))
            | (Layer::Blend(_), Layer::Blend(_))
            | (Layer::Filter { .. }, Layer::Filter { .. })
            | (Layer::Backdrop { .. }, Layer::Backdrop { .. })
    )
}

fn patch_retained_layer_ops(
    ops: &mut [ExecOp],
    id: RetainedNodeId,
    old_draw: Option<usize>,
    old_layer: Option<&Layer>,
    new: &Command,
) -> bool {
    let mut matched = false;
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                children,
                ..
            } => {
                if retained_id.is_some_and(|surface| surface.node == id)
                    && old_draw == Some(*draw)
                    && old_layer.is_some_and(|old| same_layer_plan_shape(old, layer))
                {
                    let Command::Layer {
                        draw: replacement_draw,
                        layer: replacement,
                        ..
                    } = new
                    else {
                        return false;
                    };
                    *draw = *replacement_draw;
                    *layer = replacement.clone();
                    matched = true;
                }
                matched |= patch_retained_layer_ops(children, id, old_draw, old_layer, new);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if retained_id.is_some_and(|surface| surface.node == id) {
                    let Command::MaskLayer {
                        layer: replacement, ..
                    } = new
                    else {
                        return false;
                    };
                    *layer = replacement.clone();
                    matched = true;
                }
                matched |= patch_retained_layer_ops(content, id, old_draw, old_layer, new);
                matched |= patch_retained_layer_ops(mask, id, old_draw, old_layer, new);
            }
            _ => {}
        }
    }
    // Fused layers have no retained-id-bearing ExecOp; their stack entry was patched above.
    matched
        || matches!(
            new,
            Command::Layer {
                layer: Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_),
                ..
            }
        )
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
            _ => {}
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
            _ => {}
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
            Arc::make_mut(previous_draws).extend_from_slice(draws);
            for &owner in owners.iter() {
                if !previous_owners.contains(&owner) {
                    Arc::make_mut(previous_owners).push(owner);
                }
            }
            continue;
        }
        coalesced.push(op);
    }
    *ops = coalesced;
}

#[derive(Clone, Debug)]
pub(crate) enum ExecOp {
    DrawBatch {
        draws: Arc<Vec<usize>>,
        batch_id: u32,
        owners: Arc<Vec<RetainedBatchOwner>>,
        /// Active fused layer stack for this batch in nesting order. This is
        /// stack state, not coverage.
        layer_stack: Range<usize>,
    },
    BeginClip,
    EndClip,
    BeginOpacity,
    EndOpacity,
    BeginBlend,
    EndBlend,
    OffscreenLayer {
        retained_id: Option<RetainedSurfaceId>,
        draw: usize,
        layer: Layer,
        outer_stack: Range<usize>,
        children: Vec<ExecOp>,
    },
    OffscreenMaskLayer {
        retained_id: Option<RetainedSurfaceId>,
        layer: Mask,
        outer_stack: Range<usize>,
        content: Vec<ExecOp>,
        mask: Vec<ExecOp>,
    },
}
