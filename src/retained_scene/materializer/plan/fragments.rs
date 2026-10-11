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
        while let Some(parent) = scene.nodes.get(&id)?.parent {
            if parent.node == scene.root {
                return self.root_plan_fragments.contains_key(&id).then_some(id);
            }
            id = parent.node;
        }
        None
    }

    pub(crate) fn index_root_plan_fragments(&mut self, scene: &RetainedScene) {
        let Some(root_owner_ops) = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .map(|plan| plan.root_owner_op_indices())
        else {
            return;
        };
        let layers = scene.nodes[&scene.root]
            .content
            .order
            .values()
            .copied()
            .filter(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)))
            .collect::<Vec<_>>();
        for id in layers {
            let Some(&location) = self.layer_command_locations.get(&id) else {
                continue;
            };
            let command = Rc::make_mut(&mut self.canvas).command_lists[0].commands
                [location.command_index]
                .clone();
            let temporary = self.push_command_list();
            Rc::make_mut(&mut self.canvas).command_lists[temporary]
                .commands
                .push(command);
            let fragment = Rc::make_mut(&mut self.canvas).compile(temporary);
            let _ = Rc::make_mut(&mut self.canvas).command_lists.pop();
            let Some(ops) = Rc::make_mut(&mut self.canvas)
                .compiled_plan
                .as_ref()
                .and_then(|plan| plan.root_fragment_range(&fragment, id, &root_owner_ops))
            else {
                continue;
            };
            let mut nodes = Vec::new();
            scene.collect_subtree(id, &mut nodes);
            self.root_fragment_owners
                .extend(nodes.iter().copied().map(|node| (node, id)));
            self.root_plan_fragments.insert(
                id,
                RootPlanFragment {
                    ops,
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
    }
}
