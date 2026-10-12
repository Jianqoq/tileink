use super::command_order::{Direction, command_container, command_neighbor};
use super::helpers::painter_path;
use super::*;
use crate::shared::execution::{ExecOp, ROOT_COMMAND_LIST_ID};

impl PersistentSceneMaterializer {
    /// A lack of backdrop dependencies does not imply a flat plan: Scenes can own
    /// clips and filters. Root-wide reuse needs the actual single unscoped batch.
    #[allow(
        clippy::expect_used,
        reason = "Materialized commands and the installed plan keep live location indices synchronized"
    )]
    pub(crate) fn flat_root_batch(&self) -> Option<u32> {
        if !self.dependency_free {
            return None;
        }
        match self
            .canvas
            .compiled_plan
            .as_ref()
            .expect("materialized scene owns a plan")
            .ops
            .as_slice()
        {
            [
                ExecOp::DrawBatch {
                    batch_id,
                    layer_stack,
                    ..
                },
            ] if layer_stack.is_empty() => Some(*batch_id),
            _ => None,
        }
    }

    /// Synchronizes flat retained commands after the execution plan reused stable batch storage.
    ///
    /// The plan does not read this command list again until a structural compilation, but later
    /// content/transform updates require every live leaf to have an authoritative command
    /// location. Updating only removed, moved, and inserted commands preserves that invariant
    /// without turning a one-node topology edit into a full-scene command rebuild.
    pub(crate) fn sync_flat_topology_commands(
        &mut self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) {
        // The caller selected plain runs with unchanged layer commands. Retired instances
        // still lose their old command when a new node immediately reuses the same ID.
        let mut removals = Vec::new();
        let mut insertions = HashSet::default();
        let mut patches = Vec::new();
        for &id in &changes.removed_nodes {
            if !self
                .node_metadata
                .get(&id)
                .is_some_and(|metadata| metadata.kind == NodeKindTag::Scene)
            {
                continue;
            }
            let location = &self.scene_command_locations[&id];
            let live = scene
                .nodes
                .get(&id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Scene { .. }));
            if live {
                insertions.insert(id);
            }
            removals.push((location.parent_list, location.command_index, id));
        }
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            if !matches!(node.kind, NodeKind::Scene { .. }) {
                continue;
            }
            if insertions.contains(&id) {
                continue;
            }
            let Some(previous) = self
                .node_metadata
                .get(&id)
                .filter(|old| old.kind == NodeKindTag::Scene)
            else {
                // Groups have no command of their own; a new Scene instance inserts one.
                insertions.insert(id);
                continue;
            };
            let location = &self.scene_command_locations[&id];
            if !self.scene_command_is_current(scene, id, location) {
                removals.push((location.parent_list, location.command_index, id));
                insertions.insert(id);
            } else if previous.instance != node.instance || previous.generation != node.generation {
                patches.push(id);
            }
        }

        let mut insertion_targets = HashMap::default();
        for &id in &insertions {
            let container = command_container(scene, id);
            let parent_list = self.command_list_for_container(scene, container);
            insertion_targets.insert(id, (container, parent_list));
        }

        // Descending indices keep every not-yet-removed location stable. Each removal only repairs
        // the suffix that physically shifted in its own Vec.
        removals.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        for (_, _, id) in removals {
            let location = self.scene_command_locations[&id];
            self.remove_scene_command(id, &location);
        }
        for id in patches {
            self.patch_scene_commands(scene, id);
        }

        // Reverse painter order means a run of new siblings can always insert before the sibling
        // already processed to its right, preserving order without scanning the command list.
        let mut insertions = insertions
            .into_iter()
            .map(|id| (painter_path(scene, id), id))
            .collect::<Vec<_>>();
        insertions.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (_, id) in insertions {
            let (container, parent_list) = insertion_targets[&id];
            let command_index = self
                .next_located_command(scene, id, container, parent_list)
                .map_or_else(
                    || self.canvas.command_lists[parent_list].commands.len(),
                    |location| location.command_index,
                );
            let fragment = self.install_chunk_commands(id, None);
            Rc::make_mut(&mut self.canvas).command_lists[parent_list]
                .commands
                .insert(
                    command_index,
                    Command::MaterializedRetainedScene {
                        id,
                        revision: NodeRevision::new(
                            scene.nodes[&id].instance,
                            scene.nodes[&id].generation,
                        ),
                        fragment,
                    },
                );
            self.scene_command_locations.insert(
                id,
                CommandLocation {
                    parent_list,
                    command_index,
                },
            );
            self.refresh_command_indices(parent_list, command_index);
        }
    }

    fn scene_command_is_current(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        location: &CommandLocation,
    ) -> bool {
        let container = command_container(scene, id);
        let parent_list = self.command_list_for_container(scene, container);
        if location.parent_list != parent_list {
            return false;
        }
        self.next_located_command(scene, id, container, parent_list)
            .map_or_else(
                || {
                    location.command_index + 1
                        == self.canvas.command_lists[parent_list].commands.len()
                },
                |next| next.command_index == location.command_index + 1,
            )
    }

    fn command_list_for_container(
        &self,
        scene: &RetainedScene,
        container: RetainedParent,
    ) -> usize {
        if container.node == scene.root {
            return ROOT_COMMAND_LIST_ID;
        }
        let location = self.layer_command_locations[&container.node];
        match (
            &self.canvas.command_lists[location.parent_list].commands[location.command_index],
            container.branch,
        ) {
            (Command::Layer { children, .. }, RetainedChildBranch::Content) => *children,
            (Command::MaskLayer { content, .. }, RetainedChildBranch::Content) => *content,
            (Command::MaskLayer { mask, .. }, RetainedChildBranch::Mask) => *mask,
            _ => unreachable!("validated retained container has its branch command list"),
        }
    }

    pub(super) fn next_located_command(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        container: RetainedParent,
        parent_list: usize,
    ) -> Option<CommandLocation> {
        let mut next = command_neighbor(scene, id, container, Direction::Next);
        while let Some(id) = next {
            let location = match &scene.nodes[&id].kind {
                NodeKind::Scene { .. } => self.scene_command_locations.get(&id).copied(),
                NodeKind::Layer(_) => self.layer_command_locations.get(&id).copied(),
                NodeKind::Group => None,
            };
            if location.is_some_and(|location| location.parent_list == parent_list) {
                return location;
            }
            next = command_neighbor(scene, id, container, Direction::Next);
        }
        None
    }

    fn remove_scene_command(&mut self, id: RetainedNodeId, location: &CommandLocation) {
        let removed = Rc::make_mut(&mut self.canvas).command_lists[location.parent_list]
            .commands
            .remove(location.command_index);
        assert!(
            matches!(removed, Command::MaterializedRetainedScene { id: command, .. } if command == id),
            "retained scene location must reference its own command"
        );
        self.scene_command_locations.remove(&id);
        self.refresh_command_indices(location.parent_list, location.command_index);
        self.release_command(removed);
    }

    #[allow(
        clippy::expect_used,
        reason = "Materialized commands and the installed plan keep live location indices synchronized"
    )]
    pub(super) fn refresh_command_indices(&mut self, parent_list: usize, start: usize) {
        let Self {
            canvas,
            scene_command_locations,
            layer_command_locations,
            ..
        } = self;
        for (command_index, command) in Rc::make_mut(canvas).command_lists[parent_list]
            .commands
            .iter()
            .enumerate()
            .skip(start)
        {
            match command {
                Command::MaterializedRetainedScene { id, .. } => {
                    let location = scene_command_locations
                        .get_mut(id)
                        .expect("live retained scene command has a location");
                    location.parent_list = parent_list;
                    location.command_index = command_index;
                }
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => {
                    *layer_command_locations
                        .get_mut(&key.id)
                        .expect("live retained layer command has a location") = CommandLocation {
                        parent_list,
                        command_index,
                    };
                }
                _ => unreachable!("retained container lists only contain retained commands"),
            }
        }
    }
}
