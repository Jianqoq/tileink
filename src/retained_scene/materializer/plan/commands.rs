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
        self.command_storage.reset();
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        self.append_children(scene, scene.root, RetainedChildBranch::Content, 0);
    }

    #[allow(
        clippy::expect_used,
        reason = "Command assembly traverses validated branches and updates an installed compiled plan"
    )]
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
        if matches!(scene.nodes[&id].kind, NodeKind::Group) {
            self.append_children(scene, id, RetainedChildBranch::Content, target);
            return;
        }
        let location = CommandLocation {
            parent_list: target,
            command_index: self.canvas.command_lists[target].commands.len(),
        };
        let command = self.build_node_command(scene, id, location);
        Rc::make_mut(&mut self.canvas).command_lists[target]
            .commands
            .push(command);
    }

    /// A node's command owns the exact lists allocated while building it. This also allows
    /// offscreen replacement at an existing location without a temporary parent container.
    pub(super) fn build_node_command(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        location: CommandLocation,
    ) -> Command {
        let node = &scene.nodes[&id];
        let revision = NodeRevision::new(node.instance, node.generation);
        match &node.kind {
            NodeKind::Scene { .. } => {
                let fragment = self.install_chunk_commands(id, None);
                self.scene_command_locations.insert(id, location);
                Command::MaterializedRetainedScene {
                    id,
                    revision,
                    fragment,
                }
            }
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let content = self.allocate_command_lists(1).start;
                let mask_commands = self.allocate_command_lists(1).start;
                self.append_children(scene, id, RetainedChildBranch::Content, content);
                self.append_children(scene, id, RetainedChildBranch::Mask, mask_commands);
                self.layer_command_locations.insert(id, location);
                Command::MaskLayer {
                    retained: Some(PersistentLayerKey::new(id, revision)),
                    layer: mask.clone(),
                    content,
                    mask: mask_commands,
                }
            }
            NodeKind::Layer(_) => {
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let template = chunk.canvas.command_lists[ROOT_COMMAND_LIST_ID].commands[0].clone();
                let Command::Layer { layer, .. } = template else {
                    unreachable!("non-mask layer chunk has a layer command")
                };
                let children = self.allocate_command_lists(1).start;
                self.append_children(scene, id, RetainedChildBranch::Content, children);
                self.layer_command_locations.insert(id, location);
                Command::Layer {
                    retained: Some(PersistentLayerKey::new(id, revision)),
                    layer: layer.remap_draw(draw_base),
                    children,
                }
            }
            NodeKind::Group => unreachable!("groups flatten into their parent's command list"),
        }
    }

    pub(crate) fn install_chunk_commands(
        &mut self,
        id: RetainedNodeId,
        reuse: Option<std::ops::Range<usize>>,
    ) -> std::ops::Range<usize> {
        let count = self.chunks[&id].canvas.command_lists.len();
        let fragment = match reuse {
            Some(range) if range.len() == count => range,
            old => {
                if let Some(old) = old {
                    self.release_command_lists(old);
                }
                self.allocate_command_lists(count)
            }
        };
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        let lists = &mut Rc::make_mut(&mut self.canvas).command_lists[fragment.clone()];
        for (target, source) in lists.iter_mut().zip(chunk.canvas.command_lists.iter()) {
            target.commands.clear();
            target.commands.extend(
                source
                    .commands
                    .iter()
                    .cloned()
                    .map(|command| Canvas::remap_command(command, draw_base, fragment.start)),
            );
        }
        fragment
    }

    pub(crate) fn patch_scene_commands(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        let location = self.scene_command_locations[&id];
        let Command::MaterializedRetainedScene { fragment, .. } =
            self.canvas.command_lists[location.parent_list].commands[location.command_index]
                .clone()
        else {
            unreachable!("scene location owns a scene command")
        };
        let fragment = self.install_chunk_commands(id, Some(fragment));
        let command = Command::MaterializedRetainedScene {
            id,
            revision: NodeRevision::new(scene.nodes[&id].instance, scene.nodes[&id].generation),
            fragment,
        };
        Rc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index] = command;
    }

    /// Patches translated offscreen descriptors owned by one otherwise unchanged scene leaf.
    /// Draw records and stable batch membership live in arenas and do not need plan rebuilding.
    #[allow(
        clippy::expect_used,
        reason = "Command assembly traverses validated branches and updates an installed compiled plan"
    )]
    pub(crate) fn patch_scene_position_plan(&mut self, id: RetainedNodeId) -> bool {
        let location = self.scene_command_locations[&id];
        let command =
            &self.canvas.command_lists[location.parent_list].commands[location.command_index];
        let fragment = self.canvas.compile_command(command, Vec::new());
        let plan = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_mut()
            .expect("position patches preserve the existing plan");
        Rc::make_mut(plan).patch_retained_scene_position(id, &fragment)
    }

    /// Replaces one retained layer descriptor. Content stays owned by the new command;
    /// leaving Mask releases its retired branch rather than keeping an unreachable list.
    pub(crate) fn patch_layer_command(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> (Command, Command) {
        let location = self.layer_command_locations[&id];
        let old = self.canvas.command_lists[location.parent_list].commands[location.command_index]
            .clone();
        let revision = NodeRevision::new(scene.nodes[&id].instance, scene.nodes[&id].generation);
        let command = match &scene.nodes[&id].kind {
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let (content, mask_commands) = match old.clone() {
                    Command::MaskLayer { content, mask, .. } => (content, mask),
                    Command::Layer { children, .. } => {
                        (children, self.allocate_command_lists(1).start)
                    }
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                Command::MaskLayer {
                    retained: Some(PersistentLayerKey::new(id, revision)),
                    layer: mask.clone(),
                    content,
                    mask: mask_commands,
                }
            }
            NodeKind::Layer(_) => {
                let children = match old.clone() {
                    Command::Layer { children, .. } => children,
                    Command::MaskLayer { content, mask, .. } => {
                        self.release_command_branch(mask);
                        content
                    }
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let template = chunk.canvas.command_lists[ROOT_COMMAND_LIST_ID].commands[0].clone();
                let Command::Layer { layer, .. } = template else {
                    unreachable!("non-mask retained layer chunk has one layer command")
                };
                Command::Layer {
                    retained: Some(PersistentLayerKey::new(id, revision)),
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
