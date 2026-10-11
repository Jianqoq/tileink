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
        let plan = self.canvas.compiled_plan.as_ref()?;
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
        let plan = self.canvas.compiled_plan.as_ref()?;
        let mut removed_layers = changes.removed_nodes.iter().filter(|id| {
            // Removed chunks have already been released by the time candidates are selected.
            // Layer command ownership is the stable pre-removal type/index needed here.
            self.layer_command_locations.contains_key(id)
        });
        let first = *removed_layers.next()?;
        let surviving_ancestor = |mut node| {
            let mut ancestor = plan.retained_offscreen_ancestor_of(node)?;
            while changes.removed_nodes.contains(&ancestor) {
                node = ancestor;
                ancestor = plan.retained_offscreen_ancestor_of(node)?;
            }
            scene.nodes.contains_key(&ancestor).then_some(ancestor)
        };
        let candidate = surviving_ancestor(first)?;
        removed_layers
            .all(|&layer| surviving_ancestor(layer) == Some(candidate))
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
        let Some(plan) = self.canvas.compiled_plan.as_ref() else {
            return Vec::new();
        };
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

    pub(crate) fn reorder_root_offscreen_plan(&mut self, scene: &RetainedScene) -> bool {
        let order = scene.nodes[&scene.root]
            .content
            .values()
            .copied()
            .collect::<Vec<_>>();
        let canvas = Rc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_mut() else {
            return false;
        };
        if !Rc::make_mut(plan).reorder_root_offscreen(&order) {
            return false;
        }

        let mut commands = HashMap::with_capacity_and_hasher(
            canvas.command_lists[0].commands.len(),
            Default::default(),
        );
        for command in &canvas.command_lists[0].commands {
            let id = match command {
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => key.id,
                _ => return false,
            };
            commands.insert(id, command.clone());
        }
        let Some(reordered) = order
            .iter()
            .map(|id| commands.remove(id))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if !commands.is_empty() {
            return false;
        }
        canvas.command_lists[0].commands = reordered;
        for (command_index, &id) in order.iter().enumerate() {
            self.layer_command_locations.insert(
                id,
                LayerCommandLocation {
                    parent_list: 0,
                    command_index,
                },
            );
        }
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        true
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
            .and_then(|plan| plan.retained_offscreen_stack(id))
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
        let Some(plan) = Rc::make_mut(&mut self.canvas).compiled_plan.as_mut() else {
            return false;
        };
        let Some(draw_batches) =
            Rc::make_mut(plan).replace_retained_offscreen_fragment(id, fragment)
        else {
            return false;
        };
        Rc::make_mut(&mut self.canvas).command_lists[old_location.parent_list].commands
            [old_location.command_index] = command;
        self.layer_command_locations.insert(id, old_location);

        let physical_batches = draw_batches.into_iter().collect::<HashMap<_, _>>();
        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        for &node in &nodes {
            if !matches!(scene.nodes[&node].kind, NodeKind::Scene { .. }) {
                continue;
            }
            let batch = {
                let mut batches = self
                    .node_physical_draws(node)
                    .filter_map(|draw| physical_batches.get(&draw).copied());
                batches
                    .next()
                    .filter(|&batch| batches.all(|candidate| candidate == batch))
            };
            if let Some(batch) = batch {
                self.node_batches.insert(node, batch);
            }
        }
        let subtree = nodes.into_iter().collect::<HashSet<_>>();
        self.container_batches
            .retain(|parent, _| !subtree.contains(&parent.node));
        collect_container_batches(scene, id, &self.node_batches, &mut self.container_batches);
        let retained_batches = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .unwrap()
            .retained_batch_ids
            .iter()
            .filter(|(owner, _)| subtree.contains(&owner.node))
            .map(|(owner, &batch)| {
                (
                    RetainedParent {
                        node: owner.node,
                        branch: match owner.branch {
                            RetainedBatchBranch::Content => RetainedChildBranch::Content,
                            RetainedBatchBranch::Mask => RetainedChildBranch::Mask,
                        },
                    },
                    batch,
                )
            })
            .collect::<Vec<_>>();
        self.container_batches.extend(retained_batches);
        true
    }
}
