//! Select root edits against the existing plan before mutating commands or ownership.
use super::super::command_order::{Direction, command_neighbor};
use super::*;

/// A permutation validated against the unchanged plan used by this update.
pub(crate) struct RootOffscreenReorder {
    nodes: Vec<RetainedNodeId>,
    ops: Vec<usize>,
}

impl PersistentSceneMaterializer {
    pub(crate) fn root_layer_remove_candidate(
        &self,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        // A removal-only edit cannot install another live node from the same transaction.
        if changes.removed_nodes.is_empty() || !changes.changed_nodes.is_empty() {
            return None;
        }
        self.root_plan_fragments
            .iter()
            .find_map(|(&id, fragment)| (fragment.nodes == changes.removed_nodes).then_some(id))
    }

    pub(crate) fn root_layer_add_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        // Root insertion queries current output bounds in the spatial index. Resize deliberately
        // defers that index; compile the new structure without consuming its previous extent.
        if changes.surface_changed || !changes.topology_changed || !changes.removed_nodes.is_empty()
        {
            return None;
        }
        let mut layers = changes.changed_nodes.iter().filter(|id| {
            scene.nodes.get(id).is_some_and(|node| {
                matches!(node.kind, NodeKind::Layer(_))
                    && !self.layer_command_locations.contains_key(id)
                    && node.parent == Some(RetainedParent::content(scene.root))
            })
        });
        let id = *layers.next()?;
        if layers.next().is_some()
            || !changes
                .changed_nodes
                .iter()
                .all(|changed| is_descendant_or_self(scene, *changed, id))
        {
            return None;
        }
        // This edit splits one plain execution run. A following Scene with its own
        // offscreen operations needs structural compilation; do not partially move it.
        let mut next = command_neighbor(
            scene,
            id,
            RetainedParent::content(scene.root),
            Direction::Next,
        );
        while let Some(node) = next {
            match scene.nodes[&node].kind {
                NodeKind::Layer(_) => break,
                NodeKind::Scene { .. } if !self.chunks[&node].plain_fragment => return None,
                _ => {}
            }
            next = command_neighbor(
                scene,
                node,
                RetainedParent::content(scene.root),
                Direction::Next,
            );
        }
        Some(id)
    }

    #[allow(
        clippy::expect_used,
        reason = "Root reordering uses live offscreen nodes and the installed compiled plan"
    )]
    pub(crate) fn root_offscreen_reorder_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RootOffscreenReorder> {
        if !changes.hierarchy_changed
            || !changes.removed_nodes.is_empty()
            || changes.changed_nodes.is_empty()
            || !changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_some_and(|node| {
                    matches!(node.kind, NodeKind::Layer(_))
                        && node.parent == Some(RetainedParent::content(scene.root))
                        && self.layer_command_locations.contains_key(id)
                })
            })
            || !scene.nodes[&scene.root]
                .content
                .values()
                .all(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)))
        {
            return None;
        }
        let nodes = scene.nodes[&scene.root]
            .content
            .values()
            .copied()
            .collect::<Vec<_>>();
        let ops = self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("materialized scene owns a plan")
            .root_offscreen_order(&nodes)?;
        Some(RootOffscreenReorder { nodes, ops })
    }

    #[allow(
        clippy::expect_used,
        reason = "Root reordering uses live offscreen nodes and the installed compiled plan"
    )]
    pub(crate) fn reorder_root_offscreen_plan(&mut self, order: RootOffscreenReorder) {
        let canvas = Rc::make_mut(&mut self.canvas);
        let plan = canvas
            .compiled_plan
            .as_mut()
            .expect("root reorder preserves the existing plan");
        Rc::make_mut(plan).reorder_root_offscreen(&order.ops);
        // Stable command locations identify the retained commands without another identity map.
        canvas.command_lists[ROOT_COMMAND_LIST_ID].commands = order
            .nodes
            .iter()
            .map(|id| {
                canvas.command_lists[ROOT_COMMAND_LIST_ID].commands
                    [self.layer_command_locations[id].command_index]
                    .clone()
            })
            .collect();
        for (command_index, id) in order.nodes.into_iter().enumerate() {
            self.layer_command_locations.insert(
                id,
                CommandLocation {
                    parent_list: ROOT_COMMAND_LIST_ID,
                    command_index,
                },
            );
            // Every accepted root owns one offscreen operation; retain its subtree storage.
            self.root_plan_fragments
                .get_mut(&id)
                .expect("indexed root layer")
                .ops = command_index..command_index + 1;
        }
    }
}
