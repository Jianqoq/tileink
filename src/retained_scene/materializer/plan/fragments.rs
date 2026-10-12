use super::*;

mod insert;
mod remove;

impl PersistentSceneMaterializer {
    pub(crate) fn refresh_root_fragment_membership(
        &mut self,
        scene: &RetainedScene,
        changed: &HashSet<RetainedNodeId>,
        removed: &HashSet<RetainedNodeId>,
    ) {
        for id in removed {
            if let Some(owner) = self.root_fragment_owners.remove(id)
                && let Some(fragment) = self.root_plan_fragments.get_mut(&owner)
            {
                fragment.nodes.remove(id);
            }
        }
        let mut affected = Vec::new();
        for &id in changed {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            if matches!(node.kind, NodeKind::Scene { .. }) {
                affected.push(id);
            } else {
                scene.collect_subtree(id, &mut affected);
            }
        }
        affected.sort_unstable();
        affected.dedup();
        let ownership = affected
            .into_iter()
            .map(|node| {
                let old = self.root_fragment_owners.get(&node).copied();
                let new = self.root_fragment_owner(scene, node);
                (node, old, new)
            })
            .collect::<Vec<_>>();
        for (node, old, new) in ownership {
            if old == new {
                continue;
            }
            if let Some(old) = old
                && let Some(fragment) = self.root_plan_fragments.get_mut(&old)
            {
                fragment.nodes.remove(&node);
            }
            if let Some(new) = new {
                self.root_plan_fragments
                    .get_mut(&new)
                    .expect("root fragment owner exists")
                    .nodes
                    .insert(node);
                self.root_fragment_owners.insert(node, new);
            } else {
                self.root_fragment_owners.remove(&node);
            }
        }
    }

    pub(crate) fn root_fragment_owner(
        &self,
        scene: &RetainedScene,
        mut id: RetainedNodeId,
    ) -> Option<RetainedNodeId> {
        loop {
            if self.root_plan_fragments.contains_key(&id) {
                return Some(id);
            }
            id = scene.nodes[&id].parent?.node;
        }
    }

    pub(crate) fn index_root_plan_fragments(&mut self, scene: &RetainedScene) {
        use crate::shared::execution::ExecOp;

        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        // Groups flatten into the root command list. Their layers own the same
        // independent execution ranges as direct graph-root layers.
        for command in &self.canvas.command_lists[ROOT_COMMAND_LIST_ID].commands {
            let id = match command {
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => key.id,
                _ => continue,
            };
            let mut nodes = Vec::new();
            scene.collect_subtree(id, &mut nodes);
            self.root_fragment_owners
                .extend(nodes.iter().copied().map(|node| (node, id)));
            self.root_plan_fragments.insert(
                id,
                RootPlanFragment {
                    ops: 0..0,
                    // Initial command lists can be interleaved with later root siblings. They
                    // remain stable tombstones when this fragment is removed; appended fragments
                    // still record reclaimable tail ranges.
                    command_lists: 0..0,
                    nodes: nodes.into_iter().collect(),
                    reassigned_batches: HashMap::default(),
                    removed_batch: None,
                },
            );
        }
        let plan = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("fresh retained plan");
        // Root layer operations are contiguous: fused layers have distinct hidden
        // clip/group draws, and offscreen layers own complete nested operations.
        // Index the actual plan once instead of recompiling each root subtree.
        for (index, op) in plan.ops.iter().enumerate() {
            let owner = match op {
                ExecOp::DrawBatch { owners, .. } => owners
                    .iter()
                    .find_map(|owner| self.root_fragment_owners.get(&owner.node)),
                ExecOp::OffscreenLayer { retained_id, .. }
                | ExecOp::OffscreenMaskLayer { retained_id, .. } => {
                    retained_id.and_then(|id| self.root_fragment_owners.get(&id.node))
                }
            };
            if let Some(owner) = owner {
                let fragment = self.root_plan_fragments.get_mut(owner).unwrap();
                if fragment.ops.is_empty() {
                    fragment.ops.start = index;
                }
                fragment.ops.end = index + 1;
            }
        }
    }
}
