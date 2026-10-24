use super::*;

impl PersistentSceneMaterializer {
    #[allow(
        clippy::expect_used,
        reason = "Fragment membership and command locations are built from the same installed execution plan"
    )]
    pub(crate) fn remove_root_plan_fragment(&mut self, id: RetainedNodeId) {
        // Candidate selection owns this exact root subtree and its current range.
        let fragment = self
            .root_plan_fragments
            .remove(&id)
            .expect("selected root fragment");
        let location = self.layer_command_locations[&id];
        let canvas = Rc::make_mut(&mut self.canvas);
        let plan = canvas
            .compiled_plan
            .as_mut()
            .expect("root removal preserves the existing plan");
        Rc::make_mut(plan).remove_fragment(fragment.ops.clone());
        let removed_ops = fragment.ops.clone();
        for other in self.root_plan_fragments.values_mut() {
            if other.ops.start >= removed_ops.end {
                other.ops.start -= removed_ops.len();
                other.ops.end -= removed_ops.len();
            }
        }
        let root_commands = &mut canvas.command_lists[0].commands;
        let removed_tail_command = location.command_index + 1 == root_commands.len();
        let removed = root_commands.remove(location.command_index);
        // Appended root fragments are normally removed from the tail. In that case no command
        // location can shift, so scanning every retained leaf would turn a two-node removal into
        // O(scene nodes) work. Non-tail removal still repairs every affected stable location.
        if !removed_tail_command {
            for command_location in self.scene_command_locations.values_mut() {
                if command_location.parent_list == 0
                    && command_location.command_index > location.command_index
                {
                    command_location.command_index -= 1;
                }
            }
            for layer_location in self.layer_command_locations.values_mut() {
                if layer_location.parent_list == 0
                    && layer_location.command_index > location.command_index
                {
                    layer_location.command_index -= 1;
                }
            }
        }
        if let Some((index, op)) = fragment.removed_batch {
            Rc::make_mut(plan).restore_removed_batch(index, op);
            for other in self.root_plan_fragments.values_mut() {
                if other.ops.start >= index {
                    other.ops.start += 1;
                    other.ops.end += 1;
                }
            }
        }
        self.release_command(removed);
        let restored = fragment.reassigned_batches;
        for (&node, &batch) in &restored {
            self.node_batches.insert(node, batch);
        }
        for node in fragment.nodes {
            self.root_fragment_owners.remove(&node);
            self.scene_command_locations.remove(&node);
            self.layer_command_locations.remove(&node);
            self.node_batches.remove(&node);
            self.painter_bases.remove(&node);
            self.painter_parents.remove(&node);
            self.empty_branch_batches
                .retain(|parent, _| parent.node != node);
        }
        self.update_painter_metadata(&restored.keys().copied().collect());
    }
}
