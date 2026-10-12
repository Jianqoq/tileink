//! Full plans own all derived batch classifications and root-fragment indexes.
use super::*;
use crate::shared::execution::ExecPlan;

impl PersistentSceneMaterializer {
    pub(crate) fn refresh_compiled_plan(&mut self, scene: &RetainedScene) {
        if self.canvas.compiled_plan.is_some() {
            return;
        }
        let plan = Rc::new(self.canvas.compile(ROOT_COMMAND_LIST_ID));
        // Old scalar leaf IDs and fragment ranges belong to the previous plan.
        // Install all derived ownership before any later incremental edit uses it.
        self.rebuild_batch_metadata(scene, &plan);
        Rc::make_mut(&mut self.canvas).compiled_plan = Some(plan);
        self.refresh_empty_branches(scene, scene.root);
        self.index_root_plan_fragments(scene);
    }

    fn rebuild_batch_metadata(&mut self, scene: &RetainedScene, plan: &ExecPlan) {
        self.node_batches.clear();
        for (&id, node) in &scene.nodes {
            if matches!(node.kind, NodeKind::Scene { .. }) {
                let batch = single_batch(
                    self.node_physical_draws(id)
                        .map(|draw| plan.draw_batch_ids[draw]),
                );
                if let Some(batch) = batch {
                    self.node_batches.insert(id, batch);
                }
            }
        }
        self.empty_branch_batches.clear();
        self.plan_has_batches = exec_ops_have_batches(&plan.ops);
    }

    /// Install actual per-draw membership and replace each affected leaf's classification.
    /// A leaf spanning several batches must retire its previous scalar entry.
    pub(crate) fn install_fragment_batches(
        &mut self,
        scene: &RetainedScene,
        nodes: &[RetainedNodeId],
        assignments: &[(usize, u32)],
    ) {
        let draw_capacity = self.arenas.draws.values().len();
        let canvas = Rc::make_mut(&mut self.canvas);
        let painter = &mut canvas.painter;
        painter.keys.resize(draw_capacity, PainterKey::inactive());
        painter.batch_ids.resize(draw_capacity, u32::MAX);
        for &(draw, batch) in assignments {
            if set_stable_batch(
                &mut painter.batch_ids,
                &mut painter.batch_counts,
                draw,
                batch,
            ) && let Some(changes) = &mut canvas.buffer_changes
            {
                changes.painter.push(draw..draw + 1);
            }
        }
        let batches = assignments.iter().copied().collect::<HashMap<_, _>>();
        for &id in nodes {
            if !matches!(scene.nodes[&id].kind, NodeKind::Scene { .. }) {
                continue;
            }
            match single_batch(self.node_physical_draws(id).map(|draw| batches[&draw])) {
                Some(batch) => {
                    self.node_batches.insert(id, batch);
                }
                None => {
                    self.node_batches.remove(&id);
                }
            }
        }
    }

    /// A full compiler sizes membership to the synchronized draw arena, including inactive slots.
    /// Replacing the table also covers same-length edits that renumber subsequent batches.
    #[allow(
        clippy::expect_used,
        reason = "Stable batches are synchronized only after the compiled plan is installed"
    )]
    pub(crate) fn sync_stable_batches_from_plan(&mut self) {
        let canvas = Rc::make_mut(&mut self.canvas);
        let batches = (*canvas
            .compiled_plan
            .as_ref()
            .expect("plan was refreshed before batch synchronization")
            .draw_batch_ids)
            .clone();
        let dirty = changed_value_ranges(&canvas.painter.batch_ids, &batches);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter.extend(dirty);
            changes.painter = merge_index_ranges(std::mem::take(&mut changes.painter));
        }
        canvas.painter.batch_counts = count_stable_batches(&batches);
        canvas.painter.batch_ids = batches;
    }
}

fn single_batch(mut batches: impl Iterator<Item = u32>) -> Option<u32> {
    batches
        .next()
        .filter(|&first| batches.all(|batch| batch == first))
}
