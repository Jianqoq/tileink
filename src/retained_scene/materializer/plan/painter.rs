use super::*;

impl PersistentSceneMaterializer {
    /// Iterates cached local painter order at the chunk's current physical arena base.
    ///
    /// Local order changes only when the chunk execution fingerprint changes. The base is read on
    /// every call because draw-arena compaction can relocate an otherwise unchanged chunk.
    pub(crate) fn node_physical_draws(
        &self,
        id: RetainedNodeId,
    ) -> impl ExactSizeIterator<Item = usize> + '_ {
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        chunk.local_draw_order.physical(draw_base)
    }

    pub(crate) fn rebuild_painter_metadata(&mut self, scene: &RetainedScene) {
        let old = std::mem::take(&mut Rc::make_mut(&mut self.canvas).painter);
        self.painter_bases.clear();
        let mut leaves = Vec::new();
        collect_scene_leaves(scene, scene.root, &mut leaves);
        let draw_capacity = self.arenas.draws.values().len();
        let mut keys = vec![PainterKey::inactive(); draw_capacity];
        for &id in &leaves {
            let parent = scene.nodes[&id].parent.expect("leaf has retained parent");
            let base = painter_path(scene, id);
            self.painter_bases.insert(id, base.clone());
            self.painter_parents.insert(id, parent);
            for (local_order, physical) in self.node_physical_draws(id).enumerate() {
                keys[physical] = PainterKey {
                    path: base.clone(),
                    local: local_order as u32,
                };
            }
        }
        let plan = Rc::new(
            Rc::make_mut(&mut self.canvas).compile(crate::shared::execution::ROOT_COMMAND_LIST_ID),
        );
        let mut batches = (*plan.draw_batch_ids).clone();
        Rc::make_mut(&mut self.canvas).compiled_plan = Some(plan.clone());
        batches.resize(draw_capacity, u32::MAX);
        self.node_batches.clear();
        for id in leaves {
            let batch = {
                let mut ids = self
                    .node_physical_draws(id)
                    .filter_map(|draw| (batches[draw] != u32::MAX).then_some(batches[draw]));
                ids.next()
                    .filter(|&batch| ids.all(|candidate| candidate == batch))
            };
            if let Some(batch) = batch {
                self.node_batches.insert(id, batch);
            }
        }
        self.container_batches.clear();
        collect_container_batches(
            scene,
            scene.root,
            &self.node_batches,
            &mut self.container_batches,
        );
        self.container_batches
            .extend(plan.retained_batch_ids.iter().map(|(owner, &batch)| {
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
            }));
        self.flat_plan_has_draws = exec_ops_have_batches(&plan.ops);
        let canvas = Rc::make_mut(&mut self.canvas);
        if let Some(changes) = &mut canvas.buffer_changes {
            let mut dirty = changed_value_ranges(&old.keys, &keys);
            dirty.extend(changed_value_ranges(&old.batch_ids, &batches));
            // Painter keys and stable batch IDs are uploaded through the same metadata path.
            // A topology rebuild may renumber batches without changing painter order, so both
            // arrays must contribute dirty ranges or the GPU keeps stale batch membership.
            changes.painter = merge_index_ranges(dirty);
        }
        canvas.painter = PainterMetadata {
            keys,
            batch_counts: count_stable_batches(&batches),
            batch_ids: batches,
        };
    }

    pub(crate) fn update_painter_metadata(&mut self, changed: &HashSet<RetainedNodeId>) {
        self.update_painter_metadata_with_additional(changed, &[]);
    }

    pub(crate) fn update_painter_metadata_with_additional(
        &mut self,
        changed: &HashSet<RetainedNodeId>,
        additional: &[RetainedNodeId],
    ) {
        let draw_capacity = self.arenas.draws.values().len();
        let compiled_batches = self
            .canvas
            .compiled_plan
            .as_ref()
            .map(|plan| plan.draw_batch_ids.clone());
        let canvas = Rc::make_mut(&mut self.canvas);
        let mut metadata = std::mem::take(&mut canvas.painter);
        metadata.keys.resize(draw_capacity, PainterKey::inactive());
        metadata.batch_ids.resize(draw_capacity, u32::MAX);
        let mut dirty = Vec::new();
        if let Some(changes) = &canvas.buffer_changes {
            // One journal commit can patch plan membership and then apply topology cleanup.
            // Preserve earlier painter writes so the GPU never observes only the last sub-step.
            dirty.extend(changes.painter.iter().cloned());
            for range in &changes.draws {
                metadata.keys[range.clone()].fill(PainterKey::inactive());
                for physical in range.clone() {
                    set_stable_batch(
                        &mut metadata.batch_ids,
                        &mut metadata.batch_counts,
                        physical,
                        u32::MAX,
                    );
                }
                dirty.push(range.clone());
            }
        }

        // Resize owners are unique by construction. Filter the linear scratch list against the
        // journal set to avoid a second metadata write without allocating another merged set.
        for id in changed
            .iter()
            .chain(additional.iter().filter(|id| !changed.contains(id)))
            .copied()
        {
            let Some(base) = self.painter_bases.get(&id).cloned() else {
                continue;
            };
            let batch = self.node_batches.get(&id).copied();
            let mut metadata_changed = false;
            for (local_order, physical) in self.node_physical_draws(id).enumerate() {
                let key = PainterKey {
                    path: base.clone(),
                    local: local_order as u32,
                };
                metadata_changed |= metadata.keys[physical] != key;
                metadata.keys[physical] = key;
                // A layered leaf spans batches. Reuse its per-draw compiled membership when
                // the plan is stable; a pending plan rebuild supplies new membership afterward.
                let batch = batch.or_else(|| {
                    compiled_batches
                        .as_ref()
                        .map(|batches| batches.get(physical).copied().unwrap_or(u32::MAX))
                });
                if let Some(batch) = batch {
                    metadata_changed |= set_stable_batch(
                        &mut metadata.batch_ids,
                        &mut metadata.batch_counts,
                        physical,
                        batch,
                    );
                }
            }
            if metadata_changed {
                dirty.push(self.arenas.draws.range(self.chunks[&id].draws));
            }
        }
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.painter = metadata;
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter = merge_index_ranges(dirty);
        }
    }

    pub(crate) fn refresh_compiled_plan(&mut self) {
        let canvas = Rc::make_mut(&mut self.canvas);
        if canvas.compiled_plan.is_some() {
            return;
        }
        canvas.compiled_plan = Some(Rc::new(
            canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID),
        ));
    }

    /// Synchronizes stable GPU batch membership after a full execution-plan rebuild.
    ///
    /// A non-plain retained leaf can own draws on both sides of an offscreen layer, so it has no
    /// single `node_batches` entry. Clearing that leaf's dirty draw slots before recompiling used
    /// to leave them inactive even though the rebuilt plan still referenced them. Deriving the
    /// table from the new plan is the authoritative path whenever plan structure is rebuilt and
    /// also covers same-length content mutations that renumber later batches.
    pub(crate) fn sync_stable_batches_from_plan(&mut self) {
        let canvas = Rc::make_mut(&mut self.canvas);
        let plan = canvas
            .compiled_plan
            .as_ref()
            .expect("plan was refreshed before batch synchronization");
        let mut batches = (*plan.draw_batch_ids).clone();
        batches.resize(self.arenas.draws.values().len(), u32::MAX);
        let dirty = changed_value_ranges(&canvas.painter.batch_ids, &batches);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter.extend(dirty);
            changes.painter = merge_index_ranges(std::mem::take(&mut changes.painter));
        }
        canvas.painter.batch_counts = count_stable_batches(&batches);
        canvas.painter.batch_ids = batches;
    }
}
