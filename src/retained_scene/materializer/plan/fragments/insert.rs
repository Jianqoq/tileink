use super::*;

impl PersistentSceneMaterializer {
    /// Installs one root-layer subtree as an independent persistent plan
    /// fragment. Existing root batches and their large draw lists remain shared.
    pub(crate) fn insert_root_plan_fragment(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        // The caller installs this direct root layer at the command tail. Put its
        // authoritative command at the same painter position as its plan fragment;
        // a later full compile must not replay the old append order.
        if let Some(next) = self.next_located_command(
            scene,
            id,
            RetainedParent::content(scene.root),
            ROOT_COMMAND_LIST_ID,
        ) {
            let appended = self.layer_command_locations[&id].command_index;
            Rc::make_mut(&mut self.canvas).command_lists[ROOT_COMMAND_LIST_ID].commands
                [next.command_index..=appended]
                .rotate_right(1);
            self.refresh_command_indices(ROOT_COMMAND_LIST_ID, next.command_index);
        }
        let location = self.layer_command_locations[&id];
        let command = &self.canvas.command_lists[0].commands[location.command_index];
        let fragment = self.canvas.compile_command(command, Vec::new());

        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        let node_set = nodes.iter().copied().collect::<HashSet<_>>();
        let layer_bounds = chunk_layer_influence_bounds(&self.chunks[&id]);
        let layer_painter = painter_path(scene, id);
        let next_layer = self.canvas.command_lists[ROOT_COMMAND_LIST_ID]
            .commands
            .iter()
            .skip(location.command_index + 1)
            .find_map(|command| match command {
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => Some(key.id),
                _ => None,
            });
        let next_fragment_start = next_layer.map(|id| self.root_plan_fragments[&id].ops.start);
        let end_painter = next_layer.map(|id| painter_path(scene, id));
        // Only later leaves whose pixels intersect this layer can cross the new execution
        // boundary. Querying the retained tile index makes the work proportional to affected
        // pixels instead of walking every later sibling in the scene.
        let reassigned_batches = self
            .spatial_candidates(layer_bounds)
            .into_iter()
            .filter_map(|node| {
                if !matches!(scene.nodes.get(&node)?.kind, NodeKind::Scene { .. })
                    || node_set.contains(&node)
                    || self.root_fragment_owners.contains_key(&node)
                    || self.painter_bases.get(&node)?.as_ref() <= layer_painter.as_ref()
                    || end_painter
                        .as_ref()
                        .is_some_and(|end| self.painter_bases[&node].as_ref() >= end.as_ref())
                {
                    return None;
                }
                let bounds = self.node_bounds.get(&node)?;
                (!bounds.intersect(layer_bounds).is_empty())
                    .then(|| {
                        self.node_batches
                            .get(&node)
                            .copied()
                            .map(|batch| (node, batch))
                    })
                    .flatten()
            })
            .collect::<HashMap<_, _>>();
        let reassigned_draws = reassigned_batches
            .keys()
            .flat_map(|id| self.node_physical_draws(*id))
            .collect::<Vec<_>>();
        let mut moved_per_batch = HashMap::<u32, usize>::default();
        for (&node, &batch) in &reassigned_batches {
            *moved_per_batch.entry(batch).or_default() += self.node_physical_draws(node).len();
        }
        let plan = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_mut()
            .expect("root insertion preserves the existing plan");
        let plan = Rc::make_mut(plan);
        let removed_batch = (moved_per_batch.len() == 1)
            .then(|| moved_per_batch.into_iter().next().unwrap())
            .and_then(|(batch, moved)| plan.remove_batch_if_all_moved(batch, moved));
        let mut insert_at = next_fragment_start.unwrap_or(plan.ops.len());
        if removed_batch
            .as_ref()
            .is_some_and(|(index, _)| *index < insert_at)
        {
            insert_at -= 1;
        }
        let (mut ops, draw_batches) = plan.insert_fragment(insert_at, fragment);
        let after_batch = plan.insert_plain_batch(ops.end, reassigned_draws);
        if after_batch.is_some() {
            ops.end += 1;
        }
        if let Some((removed, _)) = &removed_batch {
            for fragment in self.root_plan_fragments.values_mut() {
                if fragment.ops.start > *removed {
                    fragment.ops.start -= 1;
                    fragment.ops.end -= 1;
                }
            }
        }
        for fragment in self.root_plan_fragments.values_mut() {
            if fragment.ops.start >= ops.start {
                fragment.ops.start += ops.len();
                fragment.ops.end += ops.len();
            }
        }
        if let Some(batch) = after_batch {
            for &node in reassigned_batches.keys() {
                self.node_batches.insert(node, batch);
            }
            self.update_painter_metadata(
                &reassigned_batches.keys().copied().collect::<HashSet<_>>(),
            );
        }
        self.install_fragment_batches(scene, &nodes, &draw_batches);
        self.refresh_empty_branches(scene, id);
        self.root_fragment_owners
            .extend(nodes.iter().copied().map(|node| (node, id)));
        self.root_plan_fragments.insert(
            id,
            RootPlanFragment {
                ops,
                nodes: node_set,
                reassigned_batches,
                removed_batch,
            },
        );
    }
}
