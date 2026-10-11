use super::*;

impl PersistentSceneMaterializer {
    /// Installs one newly appended root-layer subtree as an independent persistent plan
    /// fragment. Existing root batches and their large draw lists remain shared.
    pub(crate) fn append_root_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        command_lists: std::ops::Range<usize>,
    ) -> bool {
        let location = self.layer_command_locations[&id];
        if location.parent_list != 0 {
            return false;
        }
        let command = Rc::make_mut(&mut self.canvas).command_lists[0].commands
            [location.command_index]
            .clone();
        let temporary = self.push_command_list();
        Rc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .push(command);
        let fragment = Rc::make_mut(&mut self.canvas).compile(temporary);
        let _ = Rc::make_mut(&mut self.canvas).command_lists.pop();

        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        let node_set = nodes.iter().copied().collect::<HashSet<_>>();
        let layer_bounds = chunk_layer_influence_bounds(&self.chunks[&id]);
        let layer_painter = painter_path(scene, id);
        // Only later leaves whose pixels intersect this layer can cross the new execution
        // boundary. Querying the retained tile index makes the work proportional to affected
        // pixels instead of walking every later sibling in the scene.
        let reassigned_batches = self
            .spatial_candidates(layer_bounds)
            .into_iter()
            .filter_map(|node| {
                if !matches!(scene.nodes.get(&node)?.kind, NodeKind::Scene { .. })
                    || self.root_fragment_owners.contains_key(&node)
                    || self.painter_bases.get(&node)?.as_ref() <= layer_painter.as_ref()
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
        let mut after_id = false;
        let next_fragment_start = scene.nodes[&scene.root].content.values().find_map(|child| {
            if *child == id {
                after_id = true;
                None
            } else if after_id {
                self.root_plan_fragments
                    .get(child)
                    .map(|fragment| fragment.ops.start)
            } else {
                None
            }
        });
        let Some(plan) = Rc::make_mut(&mut self.canvas).compiled_plan.as_mut() else {
            return false;
        };
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
                command_lists,
                nodes: node_set,
                reassigned_batches,
                removed_batch,
            },
        );
        true
    }
}
