use super::helpers::*;
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

    pub(crate) fn rebuild_commands(&mut self, scene: &RetainedScene) {
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.compiled_plan = None;
        canvas.command_lists.clear();
        canvas.command_lists.push(CommandList::default());
        canvas.root_commands = 0;
        canvas.command_stack.clear();
        canvas.command_stack.push(0);
        canvas.layer_stack.clear();
        canvas.persistent_root = Some(scene.root);
        self.scene_command_locations.clear();
        self.layer_command_locations.clear();
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        self.append_children(scene, scene.root, RetainedChildBranch::Content, 0);
    }

    /// Synchronizes the retained command tree without discarding an execution plan whose flat
    /// batch structure was updated in place.
    ///
    /// Plain-leaf topology updates can reuse the compiled plan because painter metadata owns the
    /// live batch membership. The command tree still has to describe the new topology: later
    /// content and transform updates patch commands through its stable node locations.
    pub(crate) fn rebuild_commands_preserving_plan(&mut self, scene: &RetainedScene) {
        let compiled_plan = self.canvas.compiled_plan.clone();
        self.rebuild_commands(scene);
        Rc::make_mut(&mut self.canvas).compiled_plan = compiled_plan;
        self.index_root_plan_fragments(scene);
    }

    pub(crate) fn append_children(
        &mut self,
        scene: &RetainedScene,
        parent: RetainedNodeId,
        branch: RetainedChildBranch,
        target: usize,
    ) {
        let children = scene.nodes[&parent]
            .children(branch)
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
                let (children, fragment_start, fragment_count) =
                    self.install_chunk_commands(id, None);
                let command_index = Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Rc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::MaterializedRetainedScene {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        children,
                    });
                self.scene_command_locations.insert(
                    id,
                    SceneCommandLocation {
                        parent_list: target,
                        command_index,
                        fragment_start,
                        fragment_count,
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
                let root = &chunk.canvas.command_lists[chunk.canvas.root_commands];
                let template = root
                    .commands
                    .first()
                    .expect("layer chunk has one root command")
                    .clone();
                let has_draws = !chunk.canvas.draw_records.is_empty();
                let children = self.push_command_list();
                self.append_children(scene, id, RetainedChildBranch::Content, children);
                let Command::Layer { draw, layer, .. } = template else {
                    unreachable!("non-mask layer chunk has a layer command")
                };
                let draw = if !has_draws { 0 } else { draw_base + draw };
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
                        draw,
                        layer,
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
    ) -> (usize, usize, usize) {
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
        (
            list_base + chunk.canvas.root_commands,
            list_base,
            list_count,
        )
    }

    pub(crate) fn patch_scene_commands(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        let old = self.scene_command_locations[&id].clone();
        let (children, fragment_start, fragment_count) = self.install_chunk_commands(
            id,
            Some(old.fragment_start..old.fragment_start + old.fragment_count),
        );
        Rc::make_mut(&mut self.canvas).command_lists[old.parent_list].commands[old.command_index] =
            Command::MaterializedRetainedScene {
                id,
                revision: NodeGeneration::new(scene.nodes[&id].generation),
                children,
            };
        self.scene_command_locations.insert(
            id,
            SceneCommandLocation {
                fragment_start,
                fragment_count,
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
                let template =
                    chunk.canvas.command_lists[chunk.canvas.root_commands].commands[0].clone();
                let Command::Layer { draw, layer, .. } = template else {
                    unreachable!("non-mask retained layer chunk has one layer command")
                };
                Command::Layer {
                    retained: Some(PersistentLayerKey::new(id, generation)),
                    draw: draw_base + draw,
                    layer,
                    children,
                }
            }
            _ => unreachable!("changed layer set only contains retained layers"),
        };
        Rc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index] = command.clone();
        (old, command)
    }

    pub(crate) fn nested_offscreen_add_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed
            || !changes.removed_nodes.is_empty()
            || !changes.changed_nodes.iter().any(|id| {
                scene
                    .nodes
                    .get(id)
                    .is_some_and(|node| matches!(node.kind, NodeKind::Layer(_)))
                    && !self.layer_command_locations.contains_key(id)
            })
        {
            return None;
        }
        let plan = self.canvas.compiled_plan.as_ref()?;
        let mut candidate = None;
        for &id in &changes.changed_nodes {
            if self.layer_command_locations.contains_key(&id)
                || self.scene_command_locations.contains_key(&id)
            {
                return None;
            }
            let mut parent = scene.nodes.get(&id)?.parent?.node;
            let ancestor = loop {
                if self.layer_command_locations.contains_key(&parent)
                    && plan.contains_retained_offscreen(parent)
                {
                    break parent;
                }
                parent = scene.nodes.get(&parent)?.parent?.node;
            };
            if candidate.is_some_and(|candidate| candidate != ancestor) {
                return None;
            }
            candidate = Some(ancestor);
        }
        candidate
    }

    pub(crate) fn nested_offscreen_remove_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed || changes.removed_nodes.is_empty() {
            return None;
        }
        let plan = self.canvas.compiled_plan.as_ref()?;
        let mut removed_layers = changes.removed_nodes.iter().filter(|id| {
            // Removed chunks have already been released by the time candidates are selected.
            // Layer command ownership is the stable pre-removal type/index needed here.
            self.layer_command_locations.contains_key(id)
        });
        let first = *removed_layers.next()?;
        let surviving_ancestor = |mut node| {
            let mut ancestor = plan.retained_offscreen_ancestor_of(node)?;
            while changes.removed_nodes.contains(&ancestor) {
                node = ancestor;
                ancestor = plan.retained_offscreen_ancestor_of(node)?;
            }
            scene.nodes.contains_key(&ancestor).then_some(ancestor)
        };
        let candidate = surviving_ancestor(first)?;
        removed_layers
            .all(|&layer| surviving_ancestor(layer) == Some(candidate))
            .then_some(candidate)
    }

    pub(crate) fn nested_offscreen_hierarchy_candidates(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Vec<RetainedNodeId> {
        if !changes.hierarchy_changed || !changes.removed_nodes.is_empty() {
            return Vec::new();
        }
        let Some(plan) = self.canvas.compiled_plan.as_ref() else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                return Vec::new();
            };
            if !matches!(node.kind, NodeKind::Layer(_))
                || !self.layer_command_locations.contains_key(&id)
            {
                return Vec::new();
            }
            let Some(old) = plan.retained_offscreen_ancestor_of(id) else {
                return Vec::new();
            };
            let mut parent = node.parent.map(|parent| parent.node);
            let new = loop {
                let Some(candidate) = parent else {
                    return Vec::new();
                };
                if self.layer_command_locations.contains_key(&candidate)
                    && plan.contains_retained_offscreen(candidate)
                {
                    break candidate;
                }
                parent = scene
                    .nodes
                    .get(&candidate)
                    .and_then(|node| node.parent.map(|p| p.node));
            };
            for candidate in [old, new] {
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    pub(crate) fn reorder_root_offscreen_plan(&mut self, scene: &RetainedScene) -> bool {
        let order = scene.nodes[&scene.root]
            .content
            .values()
            .copied()
            .collect::<Vec<_>>();
        let canvas = Rc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_mut() else {
            return false;
        };
        if !Rc::make_mut(plan).reorder_root_offscreen(&order) {
            return false;
        }

        let mut commands = HashMap::with_capacity_and_hasher(
            canvas.command_lists[0].commands.len(),
            Default::default(),
        );
        for command in &canvas.command_lists[0].commands {
            let id = match command {
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => key.id,
                _ => return false,
            };
            commands.insert(id, command.clone());
        }
        let Some(reordered) = order
            .iter()
            .map(|id| commands.remove(id))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if !commands.is_empty() {
            return false;
        }
        canvas.command_lists[0].commands = reordered;
        for (command_index, &id) in order.iter().enumerate() {
            self.layer_command_locations.insert(
                id,
                LayerCommandLocation {
                    parent_list: 0,
                    command_index,
                },
            );
        }
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        true
    }

    /// Recompiles only one retained offscreen ancestor and installs its new command/plan
    /// fragment. Unchanged branch owners keep their BatchIds; new nested branches receive fresh
    /// IDs without rewriting unrelated root operations.
    pub(crate) fn rebuild_offscreen_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> bool {
        let old_location = self.layer_command_locations[&id];
        let temporary = self.push_command_list();
        self.append_node_commands(scene, id, temporary);
        let command = Rc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .first()
            .cloned()
            .expect("offscreen fragment root command");
        let fragment = Rc::make_mut(&mut self.canvas).compile(temporary);
        let Some(plan) = Rc::make_mut(&mut self.canvas).compiled_plan.as_mut() else {
            return false;
        };
        let Some(draw_batches) =
            Rc::make_mut(plan).replace_retained_offscreen_fragment(id, fragment)
        else {
            return false;
        };
        Rc::make_mut(&mut self.canvas).command_lists[old_location.parent_list].commands
            [old_location.command_index] = command;
        self.layer_command_locations.insert(id, old_location);

        let physical_batches = draw_batches.into_iter().collect::<HashMap<_, _>>();
        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        for &node in &nodes {
            if !matches!(scene.nodes[&node].kind, NodeKind::Scene { .. }) {
                continue;
            }
            let batch = {
                let mut batches = self
                    .node_physical_draws(node)
                    .filter_map(|draw| physical_batches.get(&draw).copied());
                batches
                    .next()
                    .filter(|&batch| batches.all(|candidate| candidate == batch))
            };
            if let Some(batch) = batch {
                self.node_batches.insert(node, batch);
            }
        }
        let subtree = nodes.into_iter().collect::<HashSet<_>>();
        self.container_batches
            .retain(|parent, _| !subtree.contains(&parent.node));
        collect_container_batches(scene, id, &self.node_batches, &mut self.container_batches);
        let retained_batches = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .unwrap()
            .retained_batch_ids
            .iter()
            .filter(|(owner, _)| subtree.contains(&owner.node))
            .map(|(owner, &batch)| {
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
            })
            .collect::<Vec<_>>();
        self.container_batches.extend(retained_batches);
        true
    }

    /// Installs one newly appended root-layer subtree as an independent persistent plan
    /// fragment. Existing root batches and their large draw lists remain shared.
    pub(crate) fn append_root_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        command_lists: std::ops::Range<usize>,
    ) -> bool {
        let compile_profile = crate::wgpu::start_cpu_scope("retained.root_fragment.compile");
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
        drop(compile_profile);

        let spatial_profile = crate::wgpu::start_cpu_scope("retained.root_fragment.spatial");
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
        drop(spatial_profile);
        let plan_profile = crate::wgpu::start_cpu_scope("retained.root_fragment.plan");
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
        let retained_batches = plan
            .retained_batch_ids
            .iter()
            .filter(|(owner, _)| node_set.contains(&owner.node))
            .map(|(owner, &batch)| {
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
            })
            .collect::<Vec<_>>();
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
        drop(plan_profile);
        let metadata_profile = crate::wgpu::start_cpu_scope("retained.root_fragment.metadata");
        if let Some(batch) = after_batch {
            for &node in reassigned_batches.keys() {
                self.node_batches.insert(node, batch);
            }
            self.update_painter_metadata(
                &reassigned_batches.keys().copied().collect::<HashSet<_>>(),
            );
        }
        let physical_batches = draw_batches.into_iter().collect::<HashMap<_, _>>();
        for &node_id in &nodes {
            if !matches!(scene.nodes[&node_id].kind, NodeKind::Scene { .. }) {
                continue;
            }
            let batch = {
                let mut batches = self
                    .node_physical_draws(node_id)
                    .filter_map(|draw| physical_batches.get(&draw).copied());
                batches
                    .next()
                    .filter(|&batch| batches.all(|candidate| candidate == batch))
            };
            if let Some(batch) = batch {
                self.node_batches.insert(node_id, batch);
            }
        }
        collect_container_batches(scene, id, &self.node_batches, &mut self.container_batches);
        self.container_batches.extend(retained_batches);
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
        drop(metadata_profile);
        true
    }

    pub(crate) fn remove_root_plan_fragment(&mut self, id: RetainedNodeId) -> bool {
        let Some(fragment) = self.root_plan_fragments.remove(&id) else {
            return false;
        };
        let location = self.layer_command_locations[&id];
        let canvas = Rc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_mut() else {
            self.root_plan_fragments.insert(id, fragment);
            return false;
        };
        if location.parent_list != 0 || !Rc::make_mut(plan).remove_fragment(fragment.ops.clone()) {
            self.root_plan_fragments.insert(id, fragment);
            return false;
        }
        let removed_ops = fragment.ops.clone();
        for other in self.root_plan_fragments.values_mut() {
            if other.ops.start >= removed_ops.end {
                other.ops.start -= removed_ops.len();
                other.ops.end -= removed_ops.len();
            }
        }
        let root_commands = &mut canvas.command_lists[0].commands;
        let removed_tail_command = location.command_index + 1 == root_commands.len();
        root_commands.remove(location.command_index);
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
        if !fragment.command_lists.is_empty()
            && fragment.command_lists.end == canvas.command_lists.len()
        {
            canvas.command_lists.truncate(fragment.command_lists.start);
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
            self.container_batches
                .retain(|parent, _| parent.node != node);
        }
        self.update_painter_metadata(&restored.keys().copied().collect());
        true
    }

    pub(crate) fn refresh_root_fragment_membership(
        &mut self,
        scene: &RetainedScene,
        changed: &HashSet<RetainedNodeId>,
        removed: &HashSet<RetainedNodeId>,
    ) {
        for id in removed {
            if let Some(owner) = self.root_fragment_owners.remove(id)
                && let Some(fragment) = self.root_plan_fragments.get_mut(&owner)
            {
                fragment.nodes.remove(id);
            }
        }
        let mut affected = Vec::new();
        for &id in changed {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            if matches!(node.kind, NodeKind::Scene { .. }) {
                affected.push(id);
            } else {
                scene.collect_subtree(id, &mut affected);
            }
        }
        affected.sort_unstable();
        affected.dedup();
        let ownership = affected
            .into_iter()
            .map(|node| {
                let old = self.root_fragment_owners.get(&node).copied();
                let new = self.root_fragment_owner(scene, node);
                (node, old, new)
            })
            .collect::<Vec<_>>();
        for (node, old, new) in ownership {
            if old == new {
                continue;
            }
            if let Some(old) = old
                && let Some(fragment) = self.root_plan_fragments.get_mut(&old)
            {
                fragment.nodes.remove(&node);
            }
            if let Some(new) = new {
                self.root_plan_fragments
                    .get_mut(&new)
                    .expect("root fragment owner exists")
                    .nodes
                    .insert(node);
                self.root_fragment_owners.insert(node, new);
            } else {
                self.root_fragment_owners.remove(&node);
            }
        }
    }

    pub(crate) fn root_fragment_owner(
        &self,
        scene: &RetainedScene,
        mut id: RetainedNodeId,
    ) -> Option<RetainedNodeId> {
        while let Some(parent) = scene.nodes.get(&id)?.parent {
            if parent.node == scene.root {
                return self.root_plan_fragments.contains_key(&id).then_some(id);
            }
            id = parent.node;
        }
        None
    }

    pub(crate) fn index_root_plan_fragments(&mut self, scene: &RetainedScene) {
        let Some(root_owner_ops) = Rc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .map(|plan| plan.root_owner_op_indices())
        else {
            return;
        };
        let layers = scene.nodes[&scene.root]
            .content
            .order
            .values()
            .copied()
            .filter(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)))
            .collect::<Vec<_>>();
        for id in layers {
            let Some(&location) = self.layer_command_locations.get(&id) else {
                continue;
            };
            let command = Rc::make_mut(&mut self.canvas).command_lists[0].commands
                [location.command_index]
                .clone();
            let temporary = self.push_command_list();
            Rc::make_mut(&mut self.canvas).command_lists[temporary]
                .commands
                .push(command);
            let fragment = Rc::make_mut(&mut self.canvas).compile(temporary);
            let _ = Rc::make_mut(&mut self.canvas).command_lists.pop();
            let Some(ops) = Rc::make_mut(&mut self.canvas)
                .compiled_plan
                .as_ref()
                .and_then(|plan| plan.root_fragment_range(&fragment, id, &root_owner_ops))
            else {
                continue;
            };
            let mut nodes = Vec::new();
            scene.collect_subtree(id, &mut nodes);
            self.root_fragment_owners
                .extend(nodes.iter().copied().map(|node| (node, id)));
            self.root_plan_fragments.insert(
                id,
                RootPlanFragment {
                    ops,
                    // Initial command lists can be interleaved with later root siblings. They
                    // remain stable tombstones when this fragment is removed; appended fragments
                    // still record reclaimable tail ranges.
                    command_lists: 0..0,
                    nodes: nodes.into_iter().collect(),
                    reassigned_batches: HashMap::default(),
                    removed_batch: None,
                },
            );
        }
    }

    pub(crate) fn rebuild_painter_metadata(&mut self, scene: &RetainedScene) {
        let (old_keys, old_batches) = {
            let canvas = Rc::make_mut(&mut self.canvas);
            (canvas.painter_keys.clone(), canvas.stable_batch_ids.clone())
        };
        self.painter_bases.clear();
        let mut leaves = Vec::new();
        collect_scene_leaves(scene, scene.root, &mut leaves);
        let draw_capacity = self.arenas.draws.values().len();
        let mut keys = vec![PainterKey::inactive(); draw_capacity];
        let mut batches = vec![u32::MAX; draw_capacity];
        let mut batch_counts = Vec::new();
        for &id in &leaves {
            let parent = scene.nodes[&id].parent.expect("leaf has retained parent");
            let base = painter_path(scene, id);
            self.painter_bases.insert(id, base.clone());
            self.painter_parents.insert(id, parent);
            self.write_node_painter_metadata(
                id,
                base,
                None,
                &mut keys,
                &mut batches,
                &mut batch_counts,
            );
        }
        let plan = Rc::new(
            Rc::make_mut(&mut self.canvas).compile(crate::shared::execution::ROOT_COMMAND_LIST_ID),
        );
        batches = (*plan.draw_batch_ids).clone();
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
            let mut dirty = changed_value_ranges(old_keys.as_deref().unwrap_or(&[]), &keys);
            dirty.extend(changed_value_ranges(
                old_batches.as_deref().unwrap_or(&[]),
                &batches,
            ));
            // Painter keys and stable batch IDs are uploaded through the same metadata path.
            // A topology rebuild may renumber batches without changing painter order, so both
            // arrays must contribute dirty ranges or the GPU keeps stale batch membership.
            changes.painter = merge_index_ranges(dirty);
        }
        canvas.painter_keys = Some(keys);
        canvas.stable_batch_counts = Some(count_stable_batches(&batches));
        canvas.stable_batch_ids = Some(batches);
    }

    pub(crate) fn update_painter_metadata(&mut self, changed: &HashSet<RetainedNodeId>) {
        let draw_capacity = self.arenas.draws.values().len();
        let compiled_batches = self
            .canvas
            .compiled_plan
            .as_ref()
            .map(|plan| plan.draw_batch_ids.clone());
        let canvas = Rc::make_mut(&mut self.canvas);
        let mut keys = canvas
            .painter_keys
            .take()
            .unwrap_or_else(|| vec![PainterKey::inactive(); draw_capacity]);
        let mut batches = canvas
            .stable_batch_ids
            .take()
            .unwrap_or_else(|| vec![u32::MAX; draw_capacity]);
        let mut batch_counts = canvas
            .stable_batch_counts
            .take()
            .unwrap_or_else(|| count_stable_batches(&batches));
        keys.resize(draw_capacity, PainterKey::inactive());
        batches.resize(draw_capacity, u32::MAX);
        let mut dirty = Vec::new();
        if let Some(changes) = &canvas.buffer_changes {
            // One journal commit can patch plan membership and then apply topology cleanup.
            // Preserve earlier painter writes so the GPU never observes only the last sub-step.
            dirty.extend(changes.painter.iter().cloned());
            for range in &changes.draws {
                keys[range.clone()].fill(PainterKey::inactive());
                for physical in range.clone() {
                    set_stable_batch(&mut batches, &mut batch_counts, physical, u32::MAX);
                }
                dirty.push(range.clone());
            }
        }
        canvas.painter_keys = Some(keys);
        canvas.stable_batch_ids = Some(batches);
        canvas.stable_batch_counts = Some(batch_counts);

        for &id in changed {
            let Some(base) = self.painter_bases.get(&id).cloned() else {
                continue;
            };
            let (mut keys, mut batches, mut batch_counts) = {
                let canvas = Rc::make_mut(&mut self.canvas);
                (
                    canvas.painter_keys.take().unwrap(),
                    canvas.stable_batch_ids.take().unwrap(),
                    canvas.stable_batch_counts.take().unwrap(),
                )
            };
            let batch = self.node_batches.get(&id).copied();
            let mut metadata_changed = self.write_node_painter_metadata(
                id,
                base,
                batch,
                &mut keys,
                &mut batches,
                &mut batch_counts,
            );
            if batch.is_none()
                && let Some(compiled_batches) = &compiled_batches
            {
                // Layered leaves can span several batches and therefore have no single
                // `node_batches` entry. A same-shape canvas replacement reuses the compiled
                // plan, so restore its per-draw membership after dirty draw slots were cleared.
                // Otherwise hover paint updates leave the rect/shadow draws inactive while the
                // offscreen filter operation remains visible.
                for physical in self.node_physical_draws(id) {
                    let compiled = compiled_batches.get(physical).copied().unwrap_or(u32::MAX);
                    metadata_changed |=
                        set_stable_batch(&mut batches, &mut batch_counts, physical, compiled);
                }
            }
            if metadata_changed {
                dirty.push(self.arenas.draws.range(self.chunks[&id].draws));
            }
            let canvas = Rc::make_mut(&mut self.canvas);
            canvas.painter_keys = Some(keys);
            canvas.stable_batch_ids = Some(batches);
            canvas.stable_batch_counts = Some(batch_counts);
        }
        let canvas = Rc::make_mut(&mut self.canvas);
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
        let Some(plan) = canvas.compiled_plan.as_ref() else {
            return;
        };
        let mut batches = (*plan.draw_batch_ids).clone();
        batches.resize(self.arenas.draws.values().len(), u32::MAX);
        let dirty =
            changed_value_ranges(canvas.stable_batch_ids.as_deref().unwrap_or(&[]), &batches);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter.extend(dirty);
            changes.painter = merge_index_ranges(std::mem::take(&mut changes.painter));
        }
        canvas.stable_batch_counts = Some(count_stable_batches(&batches));
        canvas.stable_batch_ids = Some(batches);
    }

    pub(crate) fn write_node_painter_metadata(
        &self,
        id: RetainedNodeId,
        base: Rc<[u128]>,
        batch: Option<u32>,
        keys: &mut [PainterKey],
        batches: &mut [u32],
        batch_counts: &mut Vec<u32>,
    ) -> bool {
        let mut changed = false;
        for (local_order, physical) in self.node_physical_draws(id).enumerate() {
            let key = PainterKey {
                path: base.clone(),
                local: local_order as u32,
            };
            changed |= keys[physical] != key;
            keys[physical] = key;
            if let Some(batch) = batch {
                changed |= set_stable_batch(batches, batch_counts, physical, batch);
            }
        }
        changed
    }
}
