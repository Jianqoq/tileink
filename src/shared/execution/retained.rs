//! Retained offscreen identity, ancestry and root ordering.
mod layers;
mod surfaces;
use super::*;

impl ExecPlan {
    pub(crate) fn contains_retained_offscreen(&self, id: RetainedNodeId) -> bool {
        find_retained_offscreen(&self.ops, id).is_some()
    }

    pub(crate) fn retained_offscreen_stack(
        &self,
        id: RetainedNodeId,
    ) -> Option<&[LayerStackEntry]> {
        find_retained_offscreen(&self.ops, id).map(|range| &self.layer_stack_data[range.clone()])
    }

    pub(crate) fn retained_offscreen_ancestor_of(
        &self,
        owner: RetainedNodeId,
    ) -> Option<RetainedNodeId> {
        find_retained_offscreen_ancestor(&self.ops, owner, None)
    }

    pub(crate) fn root_offscreen_order(&self, order: &[RetainedNodeId]) -> Option<Vec<usize>> {
        if self.ops.len() != order.len() {
            return None;
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
                _ => return None,
            };
            // Accepted root operations come from distinct validated retained layer IDs.
            by_node.insert(node, index);
        }
        order.iter().map(|id| by_node.get(id).copied()).collect()
    }

    /// Applies the permutation selected from this plan before any structural mutation.
    pub(crate) fn reorder_root_offscreen(&mut self, indices: &[usize]) {
        self.ops = indices
            .iter()
            .map(|&index| self.ops[index].clone())
            .collect();
        self.refresh_direct_root_batch_ops();
    }
}

pub(super) fn find_retained_offscreen(ops: &[ExecOp], id: RetainedNodeId) -> Option<&Range<usize>> {
    ops.iter().find_map(|op| match op {
        ExecOp::OffscreenLayer {
            retained_id,
            outer_stack,
            children,
            ..
        } => {
            if retained_id.is_some_and(|retained| retained.node == id) {
                Some(outer_stack)
            } else {
                find_retained_offscreen(children, id)
            }
        }
        ExecOp::OffscreenMaskLayer {
            retained_id,
            outer_stack,
            content,
            mask,
            ..
        } => {
            if retained_id.is_some_and(|retained| retained.node == id) {
                Some(outer_stack)
            } else {
                find_retained_offscreen(content, id).or_else(|| find_retained_offscreen(mask, id))
            }
        }
        _ => None,
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

pub(super) fn replace_retained_offscreen(
    ops: &mut [ExecOp],
    id: RetainedNodeId,
    replacement: ExecOp,
) -> bool {
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
