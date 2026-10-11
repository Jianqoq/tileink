use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn rebuild_commands(&mut self, scene: &RetainedScene) {
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.compiled_plan = None;
        canvas.command_lists.clear();
        canvas.command_lists.push(CommandList::default());
        canvas.current_commands = ROOT_COMMAND_LIST_ID;
        canvas.layer_stack.clear();
        canvas.persistent_root = Some(scene.root);
        self.scene_command_locations.clear();
        self.layer_command_locations.clear();
        self.vacant_command_fragments.clear();
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        self.append_children(scene, scene.root, RetainedChildBranch::Content, 0);
    }

    pub(crate) fn append_children(
        &mut self,
        scene: &RetainedScene,
        parent: RetainedNodeId,
        branch: RetainedChildBranch,
        target: usize,
    ) {
        let children = scene.nodes[&parent]
            .children(RetainedParent {
                node: parent,
                branch,
            })
            .expect("validated retained branch")
            .values()
            .copied()
            .collect::<Vec<_>>();
        for child in children {
            self.append_node_commands(scene, child, target);
        }
    }

    pub(crate) fn append_node_commands(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        target: usize,
    ) {
        let node = &scene.nodes[&id];
        match &node.kind {
            NodeKind::Group => {
                self.append_children(scene, id, RetainedChildBranch::Content, target)
            }
            NodeKind::Scene { .. } => {
                let fragment = self.install_chunk_commands(id, None);
                let command_index = Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::MaterializedRetainedScene {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        children: fragment.start,
                    });
                self.scene_command_locations.insert(
                    id,
                    SceneCommandLocation {
                        parent_list: target,
                        command_index,
                        fragment_start: fragment.start,
                        fragment_count: fragment.len(),
                    },
                );
            }
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let content = self.push_command_list();
                let mask_commands = self.push_command_list();
                self.append_children(scene, id, RetainedChildBranch::Content, content);
                self.append_children(scene, id, RetainedChildBranch::Mask, mask_commands);
                let command_index = Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::MaskLayer {
                        retained: Some(PersistentLayerKey::new(
                            id,
                            NodeGeneration::new(node.generation),
                        )),
                        layer: mask.clone(),
                        content,
                        mask: mask_commands,
                    });
                self.layer_command_locations.insert(
                    id,
                    LayerCommandLocation {
                        parent_list: target,
                        command_index,
                    },
                );
            }
            NodeKind::Layer(_) => {
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let root = &chunk.canvas.command_lists[ROOT_COMMAND_LIST_ID];
                let template = root
                    .commands
                    .first()
                    .expect("layer chunk has one root command")
                    .clone();
                let children = self.push_command_list();
                self.append_children(scene, id, RetainedChildBranch::Content, children);
                let Command::Layer { layer, .. } = template else {
                    unreachable!("non-mask layer chunk has a layer command")
                };
                let command_index = Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::Layer {
                        retained: Some(PersistentLayerKey::new(
                            id,
                            NodeGeneration::new(node.generation),
                        )),
                        layer: layer.remap_draw(draw_base),
                        children,
                    });
                self.layer_command_locations.insert(
                    id,
                    LayerCommandLocation {
                        parent_list: target,
                        command_index,
                    },
                );
            }
        }
    }

    pub(crate) fn install_chunk_commands(
        &mut self,
        id: RetainedNodeId,
        reuse: Option<std::ops::Range<usize>>,
    ) -> std::ops::Range<usize> {
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        let list_count = chunk.canvas.command_lists.len();
        let list_base = reuse.filter(|range| range.len() == list_count).map_or_else(
            || Rc::make_mut(&mut self.canvas).command_lists.len(),
            |range| range.start,
        );
        let lists = chunk
            .canvas
            .command_lists
            .iter()
            .map(|list| CommandList {
                commands: list
                    .commands
                    .iter()
                    .cloned()
                    .map(|command| Canvas::remap_command(command, draw_base, list_base))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let command_lists = &mut Rc::make_mut(&mut self.canvas).command_lists;
        if list_base == command_lists.len() {
            command_lists.extend(lists);
        } else {
            command_lists[list_base..list_base + list_count].clone_from_slice(&lists);
        }
        list_base..list_base + list_count
    }

    pub(crate) fn patch_scene_commands(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        let old = self.scene_command_locations[&id].clone();
        let fragment = self.install_chunk_commands(
            id,
            Some(old.fragment_start..old.fragment_start + old.fragment_count),
        );
        Rc::make_mut(&mut self.canvas).command_lists[old.parent_list].commands[old.command_index] =
            Command::MaterializedRetainedScene {
                id,
                revision: NodeGeneration::new(scene.nodes[&id].generation),
                children: fragment.start,
            };
        self.scene_command_locations.insert(
            id,
            SceneCommandLocation {
                fragment_start: fragment.start,
                fragment_count: fragment.len(),
                ..old
            },
        );
    }

    /// Patches translated offscreen descriptors owned by one otherwise unchanged scene leaf.
    /// Draw records and stable batch membership live in arenas and do not need plan rebuilding.
    pub(crate) fn patch_scene_position_plan(&mut self, id: RetainedNodeId) -> bool {
        let location = &self.scene_command_locations[&id];
        let command = Rc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index]
            .clone();
        let temporary = self.push_command_list();
        Rc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .push(command);
        let fragment = self.canvas.compile(temporary);
        Rc::make_mut(&mut self.canvas).command_lists.pop();
        Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_mut()
            .is_some_and(|plan| Rc::make_mut(plan).patch_retained_scene_position(id, &fragment))
    }

    /// Replaces one retained layer fragment without walking or rewriting unrelated siblings.
    /// Child command lists remain stable, including the two independent mask branches.
    pub(crate) fn patch_layer_command(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> (Command, Command) {
        let location = self.layer_command_locations[&id];
        let old = Rc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index]
            .clone();
        let generation = NodeGeneration::new(scene.nodes[&id].generation);
        let command = match &scene.nodes[&id].kind {
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let (content, mask_commands) = match old.clone() {
                    Command::MaskLayer { content, mask, .. } => (content, mask),
                    Command::Layer { children, .. } => (children, self.push_command_list()),
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                Command::MaskLayer {
                    retained: Some(PersistentLayerKey::new(id, generation)),
                    layer: mask.clone(),
                    content,
                    mask: mask_commands,
                }
            }
            NodeKind::Layer(_) => {
                let children = match old.clone() {
                    Command::Layer { children, .. } => children,
                    Command::MaskLayer { content, .. } => content,
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let template = chunk.canvas.command_lists[ROOT_COMMAND_LIST_ID].commands[0].clone();
                let Command::Layer { layer, .. } = template else {
                    unreachable!("non-mask retained layer chunk has one layer command")
                };
                Command::Layer {
                    retained: Some(PersistentLayerKey::new(id, generation)),
                    layer: layer.remap_draw(draw_base),
                    children,
                }
            }
            _ => unreachable!("changed layer set only contains retained layers"),
        };
        Rc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index] = command.clone();
        (old, command)
    }
}
