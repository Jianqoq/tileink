//! Reuse only the execution run adjacent to the leaf's actual painter position.
use super::super::command_order::{Direction, command_container, command_neighbor};
use super::*;
use crate::shared::execution::{RetainedBatchBranch, RetainedBatchOwner};

impl PersistentSceneMaterializer {
    pub(crate) fn insertion_batch(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        changed: &HashSet<RetainedNodeId>,
        memo: &mut HashMap<RetainedNodeId, u32>,
    ) -> Option<u32> {
        if let Some(&batch) = memo.get(&id) {
            return Some(batch);
        }
        let container = command_container(scene, id);
        let mut run = vec![id];
        let mut sides = [None; 2];
        for (side, direction) in sides.iter_mut().zip([Direction::Previous, Direction::Next]) {
            let mut neighbor = command_neighbor(scene, id, container, direction);
            while let Some(node) = neighbor {
                if !matches!(scene.nodes[&node].kind, NodeKind::Scene { .. })
                    || !self.chunks[&node].plain_fragment
                {
                    break;
                }
                if let Some(&batch) = memo.get(&node) {
                    *side = Some(batch);
                    break;
                }
                if !changed.contains(&node)
                    && let Some(&batch) = self.node_batches.get(&node)
                {
                    *side = Some(batch);
                    break;
                }
                run.push(node);
                neighbor = command_neighbor(scene, node, container, direction);
            }
        }
        let batch = match sides {
            [Some(a), Some(b)] if a != b => None,
            [Some(batch), _] | [_, Some(batch)] => Some(batch),
            [None, None] => self.empty_branch_batches.get(&container).copied(),
        };
        if let Some(batch) = batch {
            // Cache the whole new/empty run, avoiding quadratic walks for a large insertion.
            memo.extend(run.into_iter().map(|node| (node, batch)));
        }
        batch
    }

    #[allow(
        clippy::expect_used,
        reason = "Empty-branch indexing traverses branches validated by retained scene transactions"
    )]
    pub(crate) fn refresh_empty_branches(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        let plan = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("current retained plan");
        collect_empty_branches(
            scene,
            id,
            &self.chunks,
            &plan.retained_batch_ids,
            &mut self.empty_branch_batches,
        );
        // Fragment roots are retained layers, so every ancestor now contains an execution boundary.
        let mut parent = scene.nodes[&id].parent;
        while let Some(ancestor) = parent {
            self.empty_branch_batches.remove(&ancestor);
            parent = scene.nodes[&ancestor.node].parent;
        }
    }
}

#[allow(
    clippy::expect_used,
    reason = "Empty-branch indexing traverses branches validated by retained scene transactions"
)]
fn collect_empty_branches(
    scene: &RetainedScene,
    id: RetainedNodeId,
    chunks: &HashMap<RetainedNodeId, SceneChunk>,
    retained: &std::collections::HashMap<RetainedBatchOwner, u32>,
    empty: &mut HashMap<RetainedParent, u32>,
) -> bool {
    let node = &scene.nodes[&id];
    if matches!(node.kind, NodeKind::Scene { .. }) {
        let chunk = &chunks[&id];
        return chunk.plain_fragment && chunk.local_draw_order.physical(0).len() == 0;
    }
    let mut content_empty = true;
    for branch in [RetainedChildBranch::Content, RetainedChildBranch::Mask] {
        if branch == RetainedChildBranch::Mask
            && !matches!(node.kind, NodeKind::Layer(RetainedLayerDescriptor::Mask(_)))
        {
            continue;
        }
        let parent = RetainedParent { node: id, branch };
        let mut branch_empty = true;
        for &child in node
            .children(parent)
            .expect("validated retained branch")
            .values()
        {
            // Do not short circuit: every nested layer must index its own empty branches.
            branch_empty &= collect_empty_branches(scene, child, chunks, retained, empty);
        }
        if branch_empty && matches!(node.kind, NodeKind::Layer(_)) {
            let owner = RetainedBatchOwner {
                node: id,
                branch: match branch {
                    RetainedChildBranch::Content => RetainedBatchBranch::Content,
                    RetainedChildBranch::Mask => RetainedBatchBranch::Mask,
                },
            };
            empty.insert(parent, retained[&owner]);
        } else {
            empty.remove(&parent);
        }
        if branch == RetainedChildBranch::Content {
            content_empty = branch_empty;
        }
    }
    matches!(node.kind, NodeKind::Group) && content_empty
}
