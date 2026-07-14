use super::helpers::painter_path;
use super::*;

#[derive(Clone, Copy)]
struct DetachedSceneCommand {
    fragment_start: usize,
    fragment_count: usize,
}

impl PersistentSceneMaterializer {
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
    ) -> bool {
        if changes.removed_nodes.iter().any(|id| {
            self.node_metadata
                .get(id)
                .is_some_and(|metadata| metadata.kind == NodeKindTag::Layer)
        }) || changes.changed_nodes.iter().any(|id| {
            scene
                .nodes
                .get(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Layer(_)))
        }) {
            return false;
        }

        let mut removals = Vec::new();
        let mut insertions = HashSet::default();
        let mut patches = Vec::new();
        for &id in &changes.removed_nodes {
            if scene.nodes.contains_key(&id)
                || !self
                    .node_metadata
                    .get(&id)
                    .is_some_and(|metadata| metadata.kind == NodeKindTag::Scene)
            {
                continue;
            }
            let Some(location) = self.scene_command_locations.get(&id).cloned() else {
                return false;
            };
            removals.push((location.parent_list, location.command_index, id, false));
        }
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            if !matches!(node.kind, NodeKind::Scene { .. }) {
                continue;
            }
            let Some(previous) = self.node_metadata.get(&id).copied() else {
                insertions.insert(id);
                continue;
            };
            if previous.kind != NodeKindTag::Scene {
                return false;
            }
            let Some(location) = self.scene_command_locations.get(&id).cloned() else {
                return false;
            };
            if !self.scene_command_is_current(scene, id, &location) {
                removals.push((location.parent_list, location.command_index, id, true));
                insertions.insert(id);
            } else if previous.instance != node.instance || previous.generation != node.generation {
                patches.push(id);
            }
        }

        let mut insertion_targets = HashMap::default();
        for &id in &insertions {
            let Some(container) = command_container(scene, scene.nodes[&id].parent) else {
                return false;
            };
            let Some(parent_list) = self.command_list_for_container(scene, container) else {
                return false;
            };
            insertion_targets.insert(id, (container, parent_list));
        }

        // Descending indices keep every not-yet-removed location stable. Each removal only repairs
        // the suffix that physically shifted in its own Vec.
        removals.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        let mut detached = HashMap::default();
        for (_, _, id, live) in removals {
            let location = self
                .scene_command_locations
                .get(&id)
                .cloned()
                .expect("classified retained command location remains live");
            self.remove_scene_command(id, &location);
            let command = DetachedSceneCommand {
                fragment_start: location.fragment_start,
                fragment_count: location.fragment_count,
            };
            if live {
                detached.insert(id, command);
            } else {
                self.vacant_command_fragments
                    .push(command.fragment_start..command.fragment_start + command.fragment_count);
            }
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
            let reuse = detached.remove(&id).map(|command| {
                command.fragment_start..command.fragment_start + command.fragment_count
            });
            let reuse = reuse.or_else(|| {
                let count = self.chunks[&id].canvas.command_lists.len();
                self.vacant_command_fragments
                    .iter()
                    .position(|range| range.len() == count)
                    .map(|index| self.vacant_command_fragments.swap_remove(index))
            });
            let (children, fragment_start, fragment_count) = self.install_chunk_commands(id, reuse);
            Rc::make_mut(&mut self.canvas).command_lists[parent_list]
                .commands
                .insert(
                    command_index,
                    Command::MaterializedRetainedScene {
                        id,
                        revision: NodeGeneration::new(scene.nodes[&id].generation),
                        children,
                    },
                );
            self.scene_command_locations.insert(
                id,
                SceneCommandLocation {
                    parent_list,
                    command_index,
                    fragment_start,
                    fragment_count,
                },
            );
            self.refresh_command_indices(parent_list, command_index);
        }
        debug_assert!(detached.is_empty());
        true
    }

    fn scene_command_is_current(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        location: &SceneCommandLocation,
    ) -> bool {
        let Some(container) = command_container(scene, scene.nodes[&id].parent) else {
            return false;
        };
        let Some(parent_list) = self.command_list_for_container(scene, container) else {
            return false;
        };
        if location.parent_list != parent_list
            || !matches!(
                self.canvas.command_lists[parent_list]
                    .commands
                    .get(location.command_index),
                Some(Command::MaterializedRetainedScene { id: command, .. }) if *command == id
            )
        {
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
    ) -> Option<usize> {
        if container.node == scene.root {
            return (container.branch == RetainedChildBranch::Content).then_some(0);
        }
        let location = self.layer_command_locations.get(&container.node)?;
        match (
            self.canvas.command_lists[location.parent_list]
                .commands
                .get(location.command_index)?,
            container.branch,
        ) {
            (Command::Layer { children, .. }, RetainedChildBranch::Content) => Some(*children),
            (Command::MaskLayer { content, .. }, RetainedChildBranch::Content) => Some(*content),
            (Command::MaskLayer { mask, .. }, RetainedChildBranch::Mask) => Some(*mask),
            _ => None,
        }
    }

    fn next_located_command(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        container: RetainedParent,
        parent_list: usize,
    ) -> Option<LayerCommandLocation> {
        let mut next = next_command_node(scene, id, container);
        while let Some(id) = next {
            let location = match &scene.nodes[&id].kind {
                NodeKind::Scene { .. } => {
                    self.scene_command_locations
                        .get(&id)
                        .map(|location| LayerCommandLocation {
                            parent_list: location.parent_list,
                            command_index: location.command_index,
                        })
                }
                NodeKind::Layer(_) => self.layer_command_locations.get(&id).copied(),
                NodeKind::Group => None,
            };
            if location.is_some_and(|location| location.parent_list == parent_list) {
                return location;
            }
            next = next_command_node(scene, id, container);
        }
        None
    }

    fn remove_scene_command(&mut self, id: RetainedNodeId, location: &SceneCommandLocation) {
        let removed = Rc::make_mut(&mut self.canvas).command_lists[location.parent_list]
            .commands
            .remove(location.command_index);
        assert!(
            matches!(removed, Command::MaterializedRetainedScene { id: command, .. } if command == id),
            "retained scene location must reference its own command"
        );
        self.scene_command_locations.remove(&id);
        self.refresh_command_indices(location.parent_list, location.command_index);
    }

    fn refresh_command_indices(&mut self, parent_list: usize, start: usize) {
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
                        .expect("live retained layer command has a location") =
                        LayerCommandLocation {
                            parent_list,
                            command_index,
                        };
                }
                _ => unreachable!("retained container lists only contain retained commands"),
            }
        }
    }
}

fn command_container(
    scene: &RetainedScene,
    mut parent: Option<RetainedParent>,
) -> Option<RetainedParent> {
    loop {
        let current = parent?;
        if current.node == scene.root
            || matches!(scene.nodes.get(&current.node)?.kind, NodeKind::Layer(_))
        {
            return Some(current);
        }
        let node = scene.nodes.get(&current.node)?;
        if !matches!(node.kind, NodeKind::Group) {
            return None;
        }
        parent = node.parent;
    }
}

fn first_command_node(scene: &RetainedScene, id: RetainedNodeId) -> Option<RetainedNodeId> {
    if !matches!(scene.nodes[&id].kind, NodeKind::Group) {
        return Some(id);
    }
    scene.nodes[&id]
        .content
        .values()
        .find_map(|&child| first_command_node(scene, child))
}

fn next_command_node(
    scene: &RetainedScene,
    mut id: RetainedNodeId,
    container: RetainedParent,
) -> Option<RetainedNodeId> {
    loop {
        let parent = scene.nodes.get(&id)?.parent?;
        let siblings = scene.nodes[&parent.node].children(parent.branch).ok()?;
        let key = siblings.key_of(id)?;
        if let Some(next) = siblings
            .order
            .range((std::ops::Bound::Excluded(key), std::ops::Bound::Unbounded))
            .find_map(|(_, &sibling)| first_command_node(scene, sibling))
        {
            return Some(next);
        }
        if parent == container {
            return None;
        }
        id = parent.node;
    }
}
