use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn nested_offscreen_add_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed
            || !changes.removed_nodes.is_empty()
            || !changes.changed_nodes.iter().any(|id| {
                scene
                    .nodes
                    .get(id)
                    .is_some_and(|node| matches!(node.kind, NodeKind::Layer(_)))
                    && !self.layer_command_locations.contains_key(id)
            })
        {
            return None;
        }
        let plan = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("materialized scene owns a plan");
        let mut candidate = None;
        for &id in &changes.changed_nodes {
            if self.layer_command_locations.contains_key(&id)
                || self.scene_command_locations.contains_key(&id)
            {
                return None;
            }
            let mut parent = scene.nodes.get(&id)?.parent?.node;
            let ancestor = loop {
                if self.layer_command_locations.contains_key(&parent)
                    && plan.contains_retained_offscreen(parent)
                {
                    break parent;
                }
                parent = scene.nodes.get(&parent)?.parent?.node;
            };
            if candidate.is_some_and(|candidate| candidate != ancestor) {
                return None;
            }
            candidate = Some(ancestor);
        }
        candidate
    }

    pub(crate) fn nested_offscreen_remove_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed || changes.removed_nodes.is_empty() {
            return None;
        }
        let plan = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("materialized scene owns a plan");
        let first = *changes.removed_nodes.iter().find(|id| {
            // Removed chunks have already been released by the time candidates are selected.
            // Layer command ownership is the stable pre-removal type/index needed here.
            self.layer_command_locations.contains_key(id)
        })?;
        let mut candidate = plan.retained_offscreen_ancestor_of(first)?;
        while changes.removed_nodes.contains(&candidate) {
            candidate = plan.retained_offscreen_ancestor_of(candidate)?;
        }
        // Replacing this fragment preserves its parent command position and outer stack.
        // Moving the ancestor itself must instead compile the current command hierarchy.
        let old = self.node_metadata[&candidate];
        let current = Self::scene_node_metadata(scene, candidate);
        if old.parent != current.parent || old.order_key != current.order_key {
            return None;
        }
        let was_inside = |mut id| loop {
            if id == candidate {
                return true;
            }
            let Some(parent) = self.node_metadata.get(&id).and_then(|node| node.parent) else {
                return false;
            };
            id = parent.node;
        };
        // Check the complete delta in both trees. Current ancestry alone misses old commands
        // outside the fragment when a node moves in; layer-only removal checks miss plain leaves.
        (changes.removed_nodes.iter().all(|&id| was_inside(id))
            && changes.changed_nodes.iter().all(|&id| {
                is_descendant_or_self(scene, id, candidate)
                    && (!self.node_metadata.contains_key(&id) || was_inside(id))
            }))
        .then_some(candidate)
    }

    pub(crate) fn nested_offscreen_hierarchy_candidates(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Vec<RetainedNodeId> {
        if !changes.hierarchy_changed || !changes.removed_nodes.is_empty() {
            return Vec::new();
        }
        let plan = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("materialized scene owns a plan");
        let mut candidates = Vec::new();
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                return Vec::new();
            };
            if !matches!(node.kind, NodeKind::Layer(_))
                || !self.layer_command_locations.contains_key(&id)
            {
                return Vec::new();
            }
            let Some(old) = plan.retained_offscreen_ancestor_of(id) else {
                return Vec::new();
            };
            let mut parent = node.parent.map(|parent| parent.node);
            let new = loop {
                let Some(candidate) = parent else {
                    return Vec::new();
                };
                if self.layer_command_locations.contains_key(&candidate)
                    && plan.contains_retained_offscreen(candidate)
                {
                    break candidate;
                }
                parent = scene
                    .nodes
                    .get(&candidate)
                    .and_then(|node| node.parent.map(|p| p.node));
            };
            for candidate in [old, new] {
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    /// Recompiles only one retained offscreen ancestor and installs its new command/plan
    /// fragment. Unchanged branch owners keep their BatchIds; new nested branches receive fresh
    /// IDs without rewriting unrelated root operations.
    pub(crate) fn rebuild_offscreen_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> bool {
        let Some(outer_stack) = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("offscreen patches preserve the existing plan")
            .retained_offscreen_stack(id)
            .map(<[_]>::to_vec)
        else {
            return false;
        };
        let old_location = self.layer_command_locations[&id];
        let temporary = self.push_command_list();
        self.append_node_commands(scene, id, temporary);
        let command = Rc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .first()
            .cloned()
            .expect("offscreen fragment root command");
        let fragment = self.canvas.compile_with_layer_stack(temporary, outer_stack);
        let plan = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_mut()
            .expect("temporary compilation preserves the existing plan");
        let Some(draw_batches) =
            Rc::make_mut(plan).replace_retained_offscreen_fragment(id, fragment)
        else {
            return false;
        };
        Rc::make_mut(&mut self.canvas).command_lists[old_location.parent_list].commands
            [old_location.command_index] = command;
        self.layer_command_locations.insert(id, old_location);

        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        self.install_fragment_batches(scene, &nodes, &draw_batches);
        let subtree = nodes.into_iter().collect::<HashSet<_>>();
        self.empty_branch_batches
            .retain(|parent, _| !subtree.contains(&parent.node));
        self.refresh_empty_branches(scene, id);
        true
    }
}
