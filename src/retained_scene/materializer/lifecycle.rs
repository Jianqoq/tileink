use super::helpers::*;
use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn new(scene: &RetainedScene) -> Self {
        let canvas = Rc::new(Canvas::new_persistent(
            scene.width,
            scene.height,
            scene.scale,
            scene.root,
        ));
        let mut materializer = Self {
            scene_id: scene.id,
            version: SceneVersion::INITIAL,
            canvas: Rc::clone(&canvas),
            chunks: HashMap::default(),
            arenas: MaterializedArenas::default(),
            buffer_changes_scratch: SceneBufferChanges::default(),
            surface_resized_painter_nodes: Vec::new(),
            plan_cache_key: next_plan_cache_key(),
            scene_command_locations: HashMap::default(),
            layer_command_locations: HashMap::default(),
            vacant_command_fragments: Vec::new(),
            resource_refs: HashMap::default(),
            dependency_free: false,
            layer_nodes: HashSet::default(),
            nonlocal_dependencies: HashSet::default(),
            surface_dependent_plans: HashSet::default(),
            painter_bases: HashMap::default(),
            painter_parents: HashMap::default(),
            flat_plan_has_draws: false,
            node_bounds: HashMap::default(),
            raw_node_bounds: HashMap::default(),
            bounded_node_bounds: HashMap::default(),
            bounded_raw_node_bounds: HashMap::default(),
            node_tiles: Vec::new(),
            spatial_nodes: HashSet::default(),
            raw_node_tiles: Vec::new(),
            spatial_tiles_size: (0, 0),
            surface_metadata_stale: false,
            node_batches: HashMap::default(),
            container_batches: HashMap::default(),
            root_plan_fragments: HashMap::default(),
            root_fragment_owners: HashMap::default(),
            node_metadata: HashMap::default(),
        };
        materializer.rebuild_all_with_canvas(scene, canvas);
        materializer
    }

    pub(crate) fn scene_id(&self) -> u64 {
        self.scene_id
    }

    pub(crate) fn version(&self) -> SceneVersion {
        self.version
    }

    pub(crate) fn canvas(&self) -> Rc<Canvas> {
        self.canvas.clone()
    }

    /// Consumes the scene journal and returns whether prepared CPU/GPU scene data became stale.
    /// Raster-only invalidation advances the version without forcing materialization or upload.
    /// Applies a previously collected journal snapshot without scanning the journal again.
    ///
    /// The renderer also needs the change set to decide whether its compiled plan can be reused,
    /// so accepting the snapshot here avoids merging the same journal entries twice per frame.
    pub(crate) fn update(
        &mut self,
        scene: &RetainedScene,
        changes: Option<SceneChangeSet>,
    ) -> bool {
        if self.scene_id != scene.id {
            *self = Self::new(scene);
            return true;
        }
        if self.version == scene.version {
            return false;
        }
        self.recycle_buffer_change_capacity();
        let (mut changes, journal_gap) = match changes {
            Some(changes) => (changes, false),
            None => (self.reconcile_scene_metadata(scene), true),
        };
        changes.invalidate_all |= journal_gap;
        let compactions_before = self.arena_compactions();
        let immutable_remaps_before = self.immutable_remap_compactions();
        let surface_changed = changes.surface_changed;
        if surface_changed && self.canvas.scale_factor().to_bits() != scene.scale.to_bits() {
            self.rebuild_all(scene);
            Rc::make_mut(&mut self.canvas)
                .buffer_changes
                .as_mut()
                .expect("full rebuild records scene buffer changes")
                .surface_changed = true;
            return true;
        }
        let rebuild_surface_metadata_after_update =
            !surface_changed && self.surface_metadata_stale && changes.topology_changed;
        if surface_changed {
            Rc::make_mut(&mut self.canvas).set_surface_extent(scene.width, scene.height);
        } else if self.surface_metadata_stale {
            // Continuous resize already redraws the full target. Rebuild its exact baseline once,
            // immediately before the first later incremental mutation needs it. The scene already
            // contains that mutation, while chunks and node metadata still describe the rendered
            // resize frame, so restore old revisions before computing the delta. Structural edits
            // cannot be reconstructed against the new hierarchy and rebuild after materializing.
            if !rebuild_surface_metadata_after_update {
                self.rebuild_frame_override(scene);
                self.restore_deferred_surface_baseline(&changes.changed_nodes);
                self.rebuild_spatial_index(scene);
            }
            self.surface_metadata_stale = false;
        }
        // A same-scale resize invalidates every output pixel, so neither exact frame topology nor
        // the spatial index is consulted for damage. Preserve their previous immutable pages even
        // when responsive layout inserts or removes nodes, then rebuild the exact current
        // hierarchy before the next non-resize incremental edit. This is the root fix for dense
        // responsive resize: rebuilding metadata that the full redraw cannot consume duplicated
        // the scene traversal and could let a topology delta publish the old surface extent.
        let surface_metadata_reusable = surface_changed;
        if !surface_metadata_reusable
            && self.spatial_tiles_size
                != (self.canvas.width_in_tiles(), self.canvas.height_in_tiles())
        {
            let spatial_profile =
                crate::wgpu::start_cpu_scope("retained.materialize.spatial_index");
            self.rebuild_spatial_index(scene);
            drop(spatial_profile);
        }
        let analysis_profile = crate::wgpu::start_cpu_scope("retained.materialize.analysis");
        for id in &changes.removed_nodes {
            self.layer_nodes.remove(id);
            self.nonlocal_dependencies.remove(id);
            self.surface_dependent_plans.remove(id);
        }
        if changes.topology_changed {
            for &id in &changes.changed_nodes {
                let node = scene.nodes.get(&id);
                if node.is_some_and(|node| matches!(&node.kind, NodeKind::Layer(_))) {
                    self.layer_nodes.insert(id);
                } else {
                    self.layer_nodes.remove(&id);
                }
                // Scene chunks may contain ordinary backdrop commands. Keep their existing
                // membership until rebuild_node refreshes it from the newly encoded chunk.
                if !node.is_some_and(|node| matches!(&node.kind, NodeKind::Scene { .. })) {
                    self.nonlocal_dependencies.remove(&id);
                    self.surface_dependent_plans.remove(&id);
                }
            }
        }
        let expanded_topology_changes =
            if changes.hierarchy_changed {
                let mut topology_changes = changes.clone();
                for &id in &changes.changed_nodes {
                    if scene.nodes.get(&id).is_some_and(|node| {
                        matches!(node.kind, NodeKind::Group | NodeKind::Layer(_))
                    }) {
                        let mut leaves = Vec::new();
                        collect_scene_leaves(scene, id, &mut leaves);
                        topology_changes.changed_nodes.extend(leaves);
                    }
                }
                Some(topology_changes)
            } else {
                None
            };
        let topology_changes = expanded_topology_changes.as_ref().unwrap_or(&changes);

        let scene_data_changed = surface_changed
            || changes.topology_changed
            || !changes.changed_nodes.is_empty()
            || !changes.removed_nodes.is_empty();
        let previous_frame = Rc::make_mut(&mut self.canvas).persistent_frame.clone();
        let mut delta_eligible =
            !changes.topology_changed && !surface_changed && self.dependency_free;
        let topology_delta_eligible = changes.topology_changed
            && self.dependency_free
            && previous_frame.as_ref().is_some_and(|frame| {
                changes.changed_nodes.iter().all(|id| {
                    matches!(
                        scene.nodes.get(id).map(|node| &node.kind),
                        Some(NodeKind::Group)
                    ) || frame.node_state(*id).is_none()
                })
            });
        let root_reorder_candidate = changes.topology_changed
            && changes.removed_nodes.is_empty()
            && previous_frame.as_ref().is_some_and(|frame| {
                changes.changed_nodes.iter().all(|id| {
                    frame.node_state(*id).is_some()
                        && scene.nodes.get(id).is_some_and(|node| {
                            matches!(node.kind, NodeKind::Scene { .. })
                                && node.parent == self.painter_parents.get(id).copied()
                        })
                })
            });
        let old_painter_bases = if root_reorder_candidate {
            topology_changes
                .changed_nodes
                .iter()
                .filter_map(|id| self.painter_bases.get(id).map(|base| (*id, base.clone())))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::default()
        };
        let layer_update_candidate = changes.topology_changed
            && !changes.hierarchy_changed
            && changes.removed_nodes.is_empty()
            && changes.changed_nodes == changes.changed_layers;
        let root_layer_remove_candidate = (!changes.removed_nodes.is_empty())
            .then(|| {
                self.root_plan_fragments.iter().find_map(|(&id, fragment)| {
                    let location = self.layer_command_locations.get(&id)?;
                    (fragment.nodes == changes.removed_nodes && location.parent_list == 0)
                        .then_some(id)
                })
            })
            .flatten();
        let stable_batch_candidate = !changes.topology_changed
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| {
                    matches!(node.kind, NodeKind::Group)
                        || (self.node_batches.contains_key(id)
                            && self
                                .chunks
                                .get(id)
                                .is_some_and(|chunk| chunk.plain_fragment))
                })
            });
        let mut reorder_damage = Vec::new();
        let mut plain_topology_damage = Vec::new();
        let root_painter_update = changes.topology_changed
            && self.dependency_free
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| match node.kind {
                    NodeKind::Group => node.content.is_empty(),
                    NodeKind::Scene { .. } => true,
                    NodeKind::Layer(_) => false,
                })
            });

        let flat_plan_had_draws = self.flat_plan_has_draws;
        let removed_plain_leaves = changes.removed_nodes.iter().all(|id| {
            self.chunks
                .get(id)
                .is_some_and(|chunk| chunk.plain_fragment && self.node_batches.contains_key(id))
        });
        let mut commands_dirty = changes.topology_changed && !layer_update_candidate;
        let surface_plan_changed = surface_changed && !self.surface_dependent_plans.is_empty();
        let mut plan_dirty = changes.topology_changed || surface_plan_changed;
        let mut plan_compiled_during_update = false;
        let mut layer_plan_patches = Vec::new();
        let mut layer_bounds_changes = Vec::new();
        let mut plan_layer_stack_changes = Vec::new();
        let mut layer_bounds_stable = true;
        let mut chunks_rebuilt = 0;
        let mut position_plan_patches = Vec::new();
        let mut unpatchable_plan_change = changes.topology_changed || surface_plan_changed;
        drop(analysis_profile);
        let chunk_profile = crate::wgpu::start_cpu_scope("retained.materialize.chunks");
        for id in &changes.removed_nodes {
            if let Some(chunk) = self.chunks.remove(id) {
                self.remove_chunk(chunk);
            }
        }

        for id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(id) else {
                continue;
            };
            if matches!(node.kind, NodeKind::Group)
                || self.chunks.get(id).is_some_and(|chunk| {
                    chunk.instance == node.instance && chunk.generation == node.generation
                })
            {
                continue;
            }
            let old_layer_bounds = layer_update_candidate
                .then(|| self.chunks.get(id).map(chunk_layer_influence_bounds))
                .flatten();
            let rebuilt = self.rebuild_node(scene, *id);
            plan_dirty |= rebuilt.plan_dirty;
            if rebuilt.plan_dirty && rebuilt.transform_only {
                position_plan_patches.push(*id);
            } else {
                unpatchable_plan_change |= rebuilt.plan_dirty;
            }
            chunks_rebuilt += 1;
            if !changes.topology_changed && matches!(node.kind, NodeKind::Scene { .. }) {
                self.patch_scene_commands(scene, *id);
            } else if layer_update_candidate && matches!(node.kind, NodeKind::Layer(_)) {
                let (old, new) = self.patch_layer_command(scene, *id);
                layer_plan_patches.push((*id, old, new));
                let new_layer_bounds = self.chunks.get(id).map(chunk_layer_influence_bounds);
                layer_bounds_stable &= old_layer_bounds == new_layer_bounds;
                if let (Some(old), Some(new)) = (old_layer_bounds, new_layer_bounds)
                    && old != new
                {
                    layer_bounds_changes.push((*id, old, new));
                }
            } else {
                commands_dirty = true;
            }
        }
        drop(chunk_profile);
        let mut surface_resized_painter_nodes =
            std::mem::take(&mut self.surface_resized_painter_nodes);
        surface_resized_painter_nodes.clear();
        if surface_changed {
            let resize_profile =
                crate::wgpu::start_cpu_scope("retained.materialize.resize_surface");
            self.resize_surface_chunks(scene, &mut surface_resized_painter_nodes);
            drop(resize_profile);
        }
        self.dependency_free = self.layer_nodes.is_empty() && self.nonlocal_dependencies.is_empty();
        let plan_profile = crate::wgpu::start_cpu_scope("retained.materialize.plan_sync");
        let mut plain_topology_candidate =
            changes.topology_changed && removed_plain_leaves && !root_reorder_candidate;
        let mut topology_batch_updates = Vec::new();
        if plain_topology_candidate {
            for &id in &changes.changed_nodes {
                let Some(node) = scene.nodes.get(&id) else {
                    continue;
                };
                let Some(chunk) = self.chunks.get(&id) else {
                    plain_topology_candidate = false;
                    break;
                };
                let Some(parent) = node.parent else {
                    plain_topology_candidate = false;
                    break;
                };
                let Some(&batch) = self.container_batches.get(&parent) else {
                    plain_topology_candidate = false;
                    break;
                };
                if !matches!(node.kind, NodeKind::Scene { .. }) || !chunk.plain_fragment {
                    plain_topology_candidate = false;
                    break;
                }
                topology_batch_updates.push((id, batch));
            }
        }
        if plain_topology_candidate {
            self.node_batches.extend(topology_batch_updates);
        }
        let root_layer_add_candidate = changes
            .removed_nodes
            .is_empty()
            .then(|| {
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
                Some(id)
            })
            .flatten();
        let nested_offscreen_add_candidate = root_layer_add_candidate
            .is_none()
            .then(|| self.nested_offscreen_add_candidate(scene, &changes))
            .flatten();
        let nested_offscreen_remove_candidate = root_layer_remove_candidate
            .is_none()
            .then(|| self.nested_offscreen_remove_candidate(scene, &changes))
            .flatten();
        let nested_offscreen_candidate =
            nested_offscreen_add_candidate.or(nested_offscreen_remove_candidate);
        let nested_offscreen_hierarchy_candidates = if nested_offscreen_candidate.is_none() {
            self.nested_offscreen_hierarchy_candidates(scene, &changes)
        } else {
            Vec::new()
        };
        let root_offscreen_reorder_candidate = nested_offscreen_hierarchy_candidates.is_empty()
            && changes.hierarchy_changed
            && changes.removed_nodes.is_empty()
            && !changes.changed_nodes.is_empty()
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_some_and(|node| {
                    matches!(node.kind, NodeKind::Layer(_))
                        && node.parent == Some(RetainedParent::content(scene.root))
                        && self.layer_command_locations.contains_key(id)
                })
            })
            && scene.nodes[&scene.root]
                .content
                .values()
                .all(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)));

        if !changes.topology_changed && !self.dependency_free {
            delta_eligible = changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| {
                    matches!(node.kind, NodeKind::Group)
                        || position_plan_patches.contains(id)
                        || self.fixed_translation_bounds(scene, *id).is_some()
                        || self.chunks.get(id).is_some_and(|chunk| {
                            self.raw_node_bounds.get(id).copied()
                                == Some(chunk.canvas.visual_bounds())
                        })
                })
            });
        }

        let compacted = self.arena_compactions() != compactions_before;
        if compacted {
            // Compaction remaps every live physical allocation in the affected arena. Re-encode
            // all cross-arena offsets once, then rebuild command draw references atomically.
            self.remap_all_chunks(self.immutable_remap_compactions() != immutable_remaps_before);
            commands_dirty = true;
            plan_dirty = true;
        }
        let position_plan_patched = plan_dirty
            && !unpatchable_plan_change
            && !compacted
            && !position_plan_patches.is_empty()
            && position_plan_patches
                .iter()
                .all(|&id| self.patch_scene_position_plan(id));
        if position_plan_patched {
            // The immutable plan Rc may still be owned by the renderer for the previous frame.
            // Advance the key so prepare selects this patched Rc without recompiling the scene.
            self.plan_cache_key = next_plan_cache_key();
            plan_dirty = false;
        }
        if scene_data_changed {
            let sync_profile =
                crate::wgpu::start_cpu_scope("retained.materialize.sync_canvas_data");
            self.sync_canvas_data(chunks_rebuilt, false);
            drop(sync_profile);
            Rc::make_mut(&mut self.canvas)
                .buffer_changes
                .as_mut()
                .expect("scene-data sync records buffer changes")
                .surface_changed = surface_changed;
        } else {
            Rc::make_mut(&mut self.canvas).buffer_changes = None;
        }
        if commands_dirty
            && (compacted
                || (changes.topology_changed
                    && !root_painter_update
                    && !root_reorder_candidate
                    && !plain_topology_candidate
                    && root_layer_add_candidate.is_none()
                    && root_layer_remove_candidate.is_none()
                    && nested_offscreen_candidate.is_none()
                    && nested_offscreen_hierarchy_candidates.is_empty()
                    && !root_offscreen_reorder_candidate))
        {
            self.rebuild_commands(scene);
            commands_dirty = false;
        }
        let root_layer_remove_patched = root_layer_remove_candidate
            .filter(|_| !compacted)
            .is_some_and(|id| self.remove_root_plan_fragment(id));
        if root_layer_remove_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let root_layer_plan_patched =
            root_layer_add_candidate
                .filter(|_| !compacted)
                .is_some_and(|id| {
                    let command_start = Rc::make_mut(&mut self.canvas).command_lists.len();
                    self.append_node_commands(scene, id, 0);
                    let command_end = Rc::make_mut(&mut self.canvas).command_lists.len();
                    self.append_root_plan_fragment(scene, id, command_start..command_end)
                });
        if root_layer_plan_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let nested_offscreen_plan_patched = nested_offscreen_candidate
            .filter(|_| !compacted)
            .is_some_and(|id| {
                if nested_offscreen_remove_candidate.is_some() {
                    for removed in &changes.removed_nodes {
                        self.scene_command_locations.remove(removed);
                        self.layer_command_locations.remove(removed);
                        self.node_batches.remove(removed);
                        self.painter_bases.remove(removed);
                        self.painter_parents.remove(removed);
                        self.container_batches
                            .retain(|parent, _| parent.node != *removed);
                    }
                }
                self.rebuild_offscreen_plan_fragment(scene, id)
            });
        if nested_offscreen_plan_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let nested_offscreen_hierarchy_patched = !nested_offscreen_hierarchy_candidates.is_empty()
            && !compacted
            && nested_offscreen_hierarchy_candidates
                .iter()
                .all(|&id| self.rebuild_offscreen_plan_fragment(scene, id));
        if nested_offscreen_hierarchy_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let root_offscreen_reorder_patched = root_offscreen_reorder_candidate
            && !compacted
            && self.reorder_root_offscreen_plan(scene);
        if root_offscreen_reorder_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let layer_plan_patched = layer_update_candidate
            && !compacted
            && !layer_plan_patches.is_empty()
            && Rc::make_mut(&mut self.canvas)
                .compiled_plan
                .as_mut()
                .is_some_and(|plan| {
                    let plan = Rc::make_mut(plan);
                    for (id, old, new) in &layer_plan_patches {
                        if let Command::Layer { draw, .. } = old {
                            plan_layer_stack_changes
                                .extend(plan.layer_stack_ranges_for_draw(*draw));
                        }
                        if !plan.patch_retained_layer(*id, old, new) {
                            return false;
                        }
                    }
                    true
                });
        plan_layer_stack_changes = merge_index_ranges(plan_layer_stack_changes);
        plan_compiled_during_update |= layer_plan_patched;
        // Any command topology or layer parameter change invalidates the previous immutable
        // plan before painter metadata asks Canvas to compile. Clearing it only at the end would
        // let rebuild_painter_metadata copy batch membership from the stale plan.
        if plan_dirty
            && !layer_plan_patched
            && !root_layer_plan_patched
            && !root_layer_remove_patched
            && !nested_offscreen_plan_patched
            && !nested_offscreen_hierarchy_patched
            && !root_offscreen_reorder_patched
            && !plain_topology_candidate
            && !root_painter_update
            && !root_reorder_candidate
        {
            Rc::make_mut(&mut self.canvas).compiled_plan = None;
        }
        if scene_data_changed {
            if (root_painter_update || root_reorder_candidate || plain_topology_candidate)
                && !compacted
            {
                for id in &changes.removed_nodes {
                    self.painter_bases.remove(id);
                    self.painter_parents.remove(id);
                    self.node_batches.remove(id);
                }
                for &id in &changes.changed_nodes {
                    let Some(node) = scene.nodes.get(&id) else {
                        continue;
                    };
                    if !matches!(node.kind, NodeKind::Scene { .. }) {
                        continue;
                    }
                    let base = painter_path(scene, id);
                    let parent = node.parent.expect("retained scene leaf has parent");
                    self.painter_bases.insert(id, base);
                    self.painter_parents.insert(id, parent);
                    if self.dependency_free {
                        self.node_batches.entry(id).or_insert(0);
                    }
                }
                self.update_painter_metadata_with_additional(
                    &changes.changed_nodes,
                    &surface_resized_painter_nodes,
                );
            } else if root_layer_plan_patched
                || nested_offscreen_plan_patched
                || nested_offscreen_hierarchy_patched
                || root_offscreen_reorder_patched
            {
                for &id in &topology_changes.changed_nodes {
                    let Some(node) = scene.nodes.get(&id) else {
                        continue;
                    };
                    if matches!(node.kind, NodeKind::Scene { .. }) {
                        self.painter_bases.insert(id, painter_path(scene, id));
                        self.painter_parents
                            .insert(id, node.parent.expect("retained leaf has parent"));
                    }
                }
                self.update_painter_metadata_with_additional(
                    &topology_changes.changed_nodes,
                    &surface_resized_painter_nodes,
                );
            } else if root_layer_remove_patched {
                self.update_painter_metadata_with_additional(
                    &changes.changed_nodes,
                    &surface_resized_painter_nodes,
                );
            } else if (changes.topology_changed && !layer_plan_patched) || compacted {
                self.rebuild_painter_metadata(scene);
                plan_compiled_during_update = true;
            } else if layer_plan_patched || position_plan_patched {
                // Layer parameters and hidden layer geometry do not change leaf painter keys or
                // stable batch membership. The plan fragment above already references the new
                // hidden draw slot, so cloning the scene-wide metadata arrays would be wasted.
            } else {
                let stable_batches_remain = stable_batch_candidate
                    && !plan_dirty
                    && changes.changed_nodes.iter().all(|id| {
                        scene.nodes.get(id).is_none_or(|node| {
                            matches!(node.kind, NodeKind::Group)
                                || self
                                    .chunks
                                    .get(id)
                                    .is_some_and(|chunk| chunk.plain_fragment)
                        })
                    });
                if stable_batches_remain && flat_plan_had_draws && self.flat_plan_has_draws {
                    // Content-only updates with stable physical allocations cannot change painter
                    // paths or BatchIds. The old path rewrote and compared every changed draw,
                    // making an all-node revision pay a second O(changes) metadata walk after the
                    // chunks had already been patched.
                    plan_dirty = false;
                } else {
                    self.update_painter_metadata_with_additional(
                        &changes.changed_nodes,
                        &surface_resized_painter_nodes,
                    );
                }
            }
        }
        let topology_delta_plan_reused = topology_delta_eligible
            && !root_layer_plan_patched
            && !root_layer_remove_patched
            && !nested_offscreen_plan_patched
            && !nested_offscreen_hierarchy_patched
            && !root_offscreen_reorder_patched
            && flat_plan_had_draws
            && self.flat_plan_has_draws
            && !compacted;
        if topology_delta_plan_reused {
            commands_dirty = false;
            plan_dirty = false;
        }
        let flat_topology_plan_reused =
            (plain_topology_candidate || root_painter_update) && flat_plan_had_draws && !compacted;
        if flat_topology_plan_reused {
            commands_dirty = false;
            plan_dirty = false;
            if let Some(frame) = &previous_frame {
                for &id in &changes.changed_nodes {
                    let Some(old) = frame.node_state(id) else {
                        continue;
                    };
                    let Some(chunk) = self.chunks.get(&id) else {
                        continue;
                    };
                    let new = self.influenced_bounds(scene, id, chunk.canvas.visual_bounds());
                    plain_topology_damage.push((id, old.bounds.union(new)));
                }
            }
        }
        if (topology_delta_plan_reused || flat_topology_plan_reused)
            && !self.sync_flat_topology_commands(scene, &changes)
        {
            // Candidate detection is intentionally conservative, but an unexpected layer/group
            // shape must still take the authoritative full path instead of leaving stale command
            // locations behind.
            commands_dirty = true;
            plan_dirty = true;
        }
        let root_reorder_eligible = root_reorder_candidate && chunks_rebuilt == 0 && !compacted;
        if root_reorder_eligible {
            reorder_damage = self.root_reorder_damage(&old_painter_bases, &changes.changed_nodes);
            commands_dirty = false;
            plan_dirty = false;
        }
        if commands_dirty {
            self.rebuild_commands(scene);
        }
        if plan_dirty {
            self.plan_cache_key = next_plan_cache_key();
            if !plan_compiled_during_update {
                Rc::make_mut(&mut self.canvas).compiled_plan = None;
                self.refresh_compiled_plan();
                self.sync_stable_batches_from_plan();
            }
        }
        if let Some(buffer_changes) = &mut Rc::make_mut(&mut self.canvas).buffer_changes {
            buffer_changes.plan_structure_reused =
                layer_plan_patched || root_offscreen_reorder_patched;
            buffer_changes.plan_values_patched = position_plan_patched;
            buffer_changes.plan_layer_stack = if layer_plan_patched {
                plan_layer_stack_changes
            } else {
                Vec::new()
            };
            buffer_changes.filter_resources_changed = layer_plan_patched
                && layer_plan_patches.iter().any(|(_, old, new)| {
                    command_has_filter_resources(old) || command_has_filter_resources(new)
                });
            buffer_changes.plan_fragments_rebuilt = if position_plan_patched {
                position_plan_patches.len() as u32
            } else if layer_plan_patched {
                layer_plan_patches.len() as u32
            } else if root_layer_plan_patched {
                changes.changed_nodes.len() as u32
            } else if root_layer_remove_patched {
                changes.removed_nodes.len() as u32
            } else if nested_offscreen_plan_patched {
                if nested_offscreen_remove_candidate.is_some() {
                    changes.removed_nodes.len() as u32 + 1
                } else {
                    changes.changed_nodes.len() as u32 + 1
                }
            } else if nested_offscreen_hierarchy_patched {
                topology_changes.changed_nodes.len() as u32
                    + nested_offscreen_hierarchy_candidates.len() as u32
            } else if root_offscreen_reorder_patched {
                topology_changes.changed_nodes.len() as u32
            } else if plan_dirty {
                scene.nodes.len() as u32
            } else {
                0
            };
            buffer_changes.full_scene_sync |= journal_gap;
        } else if journal_gap {
            Rc::make_mut(&mut self.canvas).buffer_changes = Some(SceneBufferChanges {
                full_scene_sync: true,
                ..Default::default()
            });
        }
        if changes.hierarchy_changed {
            self.refresh_root_fragment_membership(
                scene,
                &changes.changed_nodes,
                &changes.removed_nodes,
            );
        }
        drop(plan_profile);
        let frame_profile = crate::wgpu::start_cpu_scope("retained.materialize.frame");
        Rc::make_mut(&mut self.canvas).plan_cache_key = Some(self.plan_cache_key);
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.invalidated_bounds.clear();
        canvas.invalidate_all = changes.invalidate_all;
        for &rect in &changes.invalidated_rects {
            canvas.invalidate_rect(rect);
        }
        delta_eligible |= layer_plan_patched && layer_bounds_stable;
        let frame_patched = if rebuild_surface_metadata_after_update {
            self.rebuild_frame_override(scene);
            self.rebuild_spatial_index(scene);
            true
        } else if surface_metadata_reusable {
            self.patch_surface_frame(scene, previous_frame.clone(), &changes.changed_nodes)
        } else if layer_plan_patched
            && !layer_bounds_stable
            && self.patch_local_layer_frame_override(
                scene,
                previous_frame.clone(),
                &layer_bounds_changes,
            )
        {
            true
        } else if delta_eligible {
            self.patch_frame_override(scene, previous_frame.clone(), &changes.changed_nodes);
            true
        } else {
            (topology_delta_eligible
                || plain_topology_candidate
                || root_painter_update
                || root_layer_plan_patched
                || root_layer_remove_patched
                || nested_offscreen_plan_patched
                || nested_offscreen_hierarchy_patched
                || root_offscreen_reorder_patched)
                && self.patch_topology_frame_override(
                    scene,
                    previous_frame.clone(),
                    topology_changes,
                    &plain_topology_damage,
                )
                || (root_reorder_eligible
                    && self.patch_topology_frame_override(
                        scene,
                        previous_frame.clone(),
                        &changes,
                        &reorder_damage,
                    ))
        };
        if !frame_patched {
            self.rebuild_frame_override(scene);
            let topology_spatial_patchable = changes.topology_changed
                && topology_changes.changed_nodes.iter().all(|id| {
                    scene
                        .nodes
                        .get(id)
                        .is_none_or(|node| !matches!(node.kind, NodeKind::Layer(_)))
                })
                && changes.removed_nodes.iter().all(|id| {
                    self.node_metadata
                        .get(id)
                        .is_none_or(|node| node.kind != NodeKindTag::Layer)
                });
            let spatial_patched = !surface_changed
                && if changes.topology_changed {
                    topology_spatial_patchable
                        && self.patch_changed_spatial_index(
                            scene,
                            &topology_changes.changed_nodes,
                            &changes.removed_nodes,
                        )
                } else {
                    self.patch_changed_spatial_index(
                        scene,
                        &changes.changed_nodes,
                        &changes.removed_nodes,
                    )
                };
            if !spatial_patched {
                self.rebuild_spatial_index(scene);
            }
        }
        drop(frame_profile);
        self.sync_node_metadata(scene, &changes);
        self.surface_resized_painter_nodes = surface_resized_painter_nodes;
        self.version = scene.version;
        scene_data_changed
    }

    pub(crate) fn rebuild_all(&mut self, scene: &RetainedScene) {
        let canvas = Rc::new(Canvas::new_persistent(
            scene.width,
            scene.height,
            scene.scale,
            scene.root,
        ));
        self.rebuild_all_with_canvas(scene, canvas);
    }

    fn rebuild_all_with_canvas(&mut self, scene: &RetainedScene, canvas: Rc<Canvas>) {
        self.scene_id = scene.id;
        self.version = scene.version;
        self.chunks.clear();
        self.arenas = MaterializedArenas::default();
        self.plan_cache_key = next_plan_cache_key();
        self.scene_command_locations.clear();
        self.resource_refs.clear();
        self.painter_bases.clear();
        self.painter_parents.clear();
        self.flat_plan_has_draws = false;
        self.node_bounds.clear();
        self.raw_node_bounds.clear();
        self.bounded_node_bounds.clear();
        self.bounded_raw_node_bounds.clear();
        self.node_tiles.clear();
        self.spatial_nodes.clear();
        self.raw_node_tiles.clear();
        self.spatial_tiles_size = (0, 0);
        self.surface_metadata_stale = false;
        self.node_batches.clear();
        self.container_batches.clear();
        self.root_plan_fragments.clear();
        self.root_fragment_owners.clear();
        self.node_metadata.clear();
        self.canvas = canvas;
        self.layer_nodes = scene
            .nodes
            .iter()
            .filter_map(|(&id, node)| matches!(&node.kind, NodeKind::Layer(_)).then_some(id))
            .collect();
        self.nonlocal_dependencies.clear();
        self.surface_dependent_plans.clear();
        for (&id, node) in &scene.nodes {
            if !matches!(node.kind, NodeKind::Group) {
                self.rebuild_node(scene, id);
            }
        }
        self.dependency_free = self.layer_nodes.is_empty() && self.nonlocal_dependencies.is_empty();
        self.sync_canvas_data(self.chunks.len() as u32, true);
        self.rebuild_commands(scene);
        self.rebuild_painter_metadata(scene);
        self.index_root_plan_fragments(scene);
        Rc::make_mut(&mut self.canvas)
            .buffer_changes
            .as_mut()
            .expect("full sync records scene buffer changes")
            .plan_fragments_rebuilt = scene.nodes.len() as u32;
        Rc::make_mut(&mut self.canvas).plan_cache_key = Some(self.plan_cache_key);
        self.rebuild_frame_override(scene);
        self.rebuild_spatial_index(scene);
        self.node_metadata = scene
            .nodes
            .keys()
            .copied()
            .map(|id| (id, Self::scene_node_metadata(scene, id)))
            .collect();
    }

    pub(crate) fn reconcile_scene_metadata(&self, scene: &RetainedScene) -> SceneChangeSet {
        let mut changes = SceneChangeSet {
            invalidate_all: true,
            surface_changed: self.canvas.logical_width != scene.width
                || self.canvas.logical_height != scene.height
                || self.canvas.scale_factor().to_bits() != scene.scale.to_bits(),
            ..Default::default()
        };
        changes.removed_nodes.extend(
            self.node_metadata
                .keys()
                .filter(|id| !scene.nodes.contains_key(id))
                .copied(),
        );
        if !changes.removed_nodes.is_empty() {
            changes.topology_changed = true;
            changes.hierarchy_changed = true;
        }
        for &id in scene.nodes.keys() {
            let current = Self::scene_node_metadata(scene, id);
            let Some(previous) = self.node_metadata.get(&id).copied() else {
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                continue;
            };
            if current == previous {
                continue;
            }
            changes.changed_nodes.insert(id);
            if current.instance != previous.instance
                || current.parent != previous.parent
                || current.order_key != previous.order_key
                || current.kind != previous.kind
            {
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
            } else if current.kind == NodeKindTag::Layer
                && current.generation != previous.generation
            {
                changes.changed_layers.insert(id);
                changes.topology_changed = true;
            }
            if current.instance != previous.instance && current.kind == NodeKindTag::Layer {
                changes.changed_layers.insert(id);
            }
        }
        changes
    }

    pub(crate) fn sync_node_metadata(&mut self, scene: &RetainedScene, changes: &SceneChangeSet) {
        for id in &changes.removed_nodes {
            self.node_metadata.remove(id);
        }
        for &id in &changes.changed_nodes {
            if scene.nodes.contains_key(&id) {
                self.node_metadata
                    .insert(id, Self::scene_node_metadata(scene, id));
            }
        }
    }

    pub(crate) fn scene_node_metadata(
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> MaterializedNodeMetadata {
        let node = &scene.nodes[&id];
        let order_key = node.parent.map(|parent| {
            scene.nodes[&parent.node]
                .children(parent.branch)
                .expect("validated retained branch")
                .key_of(id)
                .expect("retained child has an order key")
        });
        MaterializedNodeMetadata {
            instance: node.instance,
            generation: node.generation,
            parent: node.parent,
            order_key,
            kind: match &node.kind {
                NodeKind::Group => NodeKindTag::Group,
                NodeKind::Scene { .. } => NodeKindTag::Scene,
                NodeKind::Layer(_) => NodeKindTag::Layer,
            },
        }
    }

    /// Applies a same-scale viewport resize without re-encoding retained node geometry.
    ///
    /// Path backdrop and segment allocations depend on viewport clipping, so only those
    /// allocations are resized and remapped. SDF, image, and text chunks keep their stable slots.
    /// Changed and removed chunks must be processed first: resizing their stale previous-frame
    /// geometry can shrink shared scan allocations before their replacement is installed and
    /// publish an internally inconsistent frame. The regular update tail recompiles the
    /// surface-dependent plan and damage metadata.
    pub(crate) fn resize_surface_chunks(
        &mut self,
        scene: &RetainedScene,
        painter_nodes: &mut Vec<RetainedNodeId>,
    ) {
        let Self { chunks, arenas, .. } = self;
        for (&id, chunk) in chunks.iter_mut() {
            if chunk.canvas.logical_size() == (scene.width, scene.height) {
                continue;
            }
            let old_backdrops = chunk.canvas.backdrop_pool_capacity as usize;
            let old_segments = chunk.canvas.tile_cnt as usize;
            chunk.canvas.resize_surface(scene.width, scene.height);
            let new_backdrops = chunk.canvas.backdrop_pool_capacity as usize;
            let new_segments = chunk.canvas.tile_cnt as usize;
            if old_backdrops != 0 || new_backdrops != 0 {
                arenas.backdrops.resize(chunk.backdrops, new_backdrops);
            }
            if old_segments != 0 || new_segments != 0 {
                arenas.segments.resize(chunk.segments, new_segments);
            }
            if !chunk.canvas.path_records.is_empty() {
                Self::remap_chunk_data(arenas, chunk, false);
                // Resizing path scan allocations rewrites DrawRecord offsets without changing
                // painter identity. Include the owner in the normal metadata restore pass so
                // dirty GPU ranges are not mistaken for removed draws.
                painter_nodes.push(id);
            }
        }
    }

    /// Re-encodes one node and classifies whether its execution plan needs synchronization.
    pub(crate) fn rebuild_node(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> NodeRebuild {
        let node = &scene.nodes[&id];
        let (source_canvas, transform_bits) = scene_node_placement(node);
        let updated = {
            // Borrow the materializer fields independently so an existing chunk stays in its map
            // slot while encoding, arena remapping, and resource refcounts are updated.
            let Self {
                canvas,
                chunks,
                arenas,
                resource_refs,
                ..
            } = self;
            chunks.get_mut(&id).map(|chunk| {
                let transform_only = chunk
                    .source_canvas
                    .as_ref()
                    .zip(source_canvas.as_ref())
                    .is_some_and(|(old, new)| Rc::ptr_eq(old, new))
                    && chunk.transform_bits != transform_bits
                    && source_canvas.is_some();
                if transform_only {
                    let transform = Affine::new(transform_bits.unwrap().map(f64::from_bits));
                    chunk.canvas.set_retained_transform(transform);
                    let old_plan = chunk.plan_fingerprint;
                    let new_plan = chunk.canvas.execution_plan_fingerprint();
                    let mut moved = false;
                    if !chunk.canvas.path_records.is_empty() {
                        moved |= arenas.backdrops.resize(
                            chunk.backdrops,
                            chunk.canvas.backdrop_pool_capacity as usize,
                        );
                        moved |= arenas
                            .segments
                            .resize(chunk.segments, chunk.canvas.tile_cnt as usize);
                    }
                    chunk.instance = node.instance;
                    chunk.generation = node.generation;
                    chunk.transform_bits = transform_bits;
                    if old_plan != new_plan {
                        let metadata = chunk_plan_metadata(&chunk.canvas);
                        chunk.plain_fragment = metadata.plain_fragment;
                        chunk.local_draw_order = metadata.local_draw_order;
                    }
                    chunk.plan_fingerprint = new_plan;
                    if !chunk.backdrop_dependencies.is_empty() {
                        chunk.backdrop_dependencies = backdrop_dependencies(&chunk.canvas);
                    }
                    // Only transform-bearing path/draw records and scan allocation metadata
                    // changed. Local lines, paint/SDF blobs, glyphs, and runs remain immutable.
                    Self::remap_chunk_data(arenas, chunk, false);
                    return (
                        NodeRebuild {
                            plan_dirty: moved || old_plan != new_plan,
                            transform_only: !moved,
                        },
                        false,
                    );
                }

                let old_plan = chunk.plan_fingerprint;
                let old_lengths = SceneChunkLengths::from_canvas(&chunk.canvas);
                Self::remove_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                Self::encode_node_into(scene, node, &mut chunk.canvas);
                let encoded = &chunk.canvas;
                let new_lengths = SceneChunkLengths::from_canvas(encoded);
                let new_plan = encoded.execution_plan_fingerprint();
                let mut moved = false;
                if old_lengths.lines != 0 || new_lengths.lines != 0 {
                    moved |= arenas.lines.resize(chunk.lines, new_lengths.lines);
                }
                if old_lengths.paths != 0 || new_lengths.paths != 0 {
                    moved |= arenas.paths.resize(chunk.paths, new_lengths.paths);
                }
                if old_lengths.draws != 0 || new_lengths.draws != 0 {
                    moved |= arenas.draws.resize(chunk.draws, new_lengths.draws);
                }
                if old_lengths.brushes != 0 || new_lengths.brushes != 0 {
                    moved |= arenas.brushes.replace(chunk.brushes, &encoded.brush_blob);
                }
                if old_lengths.sdfs != 0 || new_lengths.sdfs != 0 {
                    moved |= arenas.sdfs.replace(chunk.sdfs, &encoded.sdf_blob);
                }
                if old_lengths.shadows != 0 || new_lengths.shadows != 0 {
                    moved |= arenas
                        .shadows
                        .replace(chunk.shadows, &encoded.sdf_shadow_blob);
                }
                moved |= Self::replace_glyphs(arenas, &mut chunk.glyphs, &encoded.text_glyphs);
                if old_lengths.runs != 0 || new_lengths.runs != 0 {
                    moved |= arenas.runs.resize(chunk.runs.unwrap(), new_lengths.runs);
                }
                if old_lengths.backdrops != 0 || new_lengths.backdrops != 0 {
                    moved |=
                        arenas
                            .backdrops
                            .replace_filled(chunk.backdrops, new_lengths.backdrops, 0);
                }
                if old_lengths.segments != 0 || new_lengths.segments != 0 {
                    moved |=
                        arenas
                            .segments
                            .replace_filled(chunk.segments, new_lengths.segments, 0);
                }
                chunk.instance = node.instance;
                chunk.generation = node.generation;
                chunk.source_canvas = source_canvas.clone();
                chunk.transform_bits = transform_bits;
                if old_plan != new_plan {
                    let metadata = chunk_plan_metadata(&chunk.canvas);
                    chunk.plain_fragment = metadata.plain_fragment;
                    chunk.local_draw_order = metadata.local_draw_order;
                }
                chunk.plan_fingerprint = new_plan;
                chunk.backdrop_dependencies = backdrop_dependencies(&chunk.canvas);
                Self::add_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                Self::remap_chunk_data(arenas, chunk, true);
                (
                    NodeRebuild {
                        plan_dirty: moved || old_plan != new_plan,
                        transform_only: false,
                    },
                    true,
                )
            })
        };
        if let Some((updated, dependencies_changed)) = updated {
            if dependencies_changed {
                self.refresh_chunk_dependencies(id);
            }
            return updated;
        }

        let encoded = Self::encode_node(scene, node);
        let glyphs = Self::insert_glyphs(&mut self.arenas, &encoded.text_glyphs);
        let plan_fingerprint = encoded.execution_plan_fingerprint();
        let plan_metadata = chunk_plan_metadata(&encoded);
        let chunk = SceneChunk {
            instance: node.instance,
            generation: node.generation,
            source_canvas,
            transform_bits,
            lines: self.arenas.lines.insert(&encoded.lines),
            paths: self.arenas.paths.insert(&encoded.path_records),
            draws: self.arenas.draws.insert(&encoded.draw_records),
            brushes: self.arenas.brushes.insert(&encoded.brush_blob),
            sdfs: self.arenas.sdfs.insert(&encoded.sdf_blob),
            shadows: self.arenas.shadows.insert(&encoded.sdf_shadow_blob),
            glyphs,
            runs: Some(self.arenas.runs.insert(&encoded.text_runs)),
            backdrops: self
                .arenas
                .backdrops
                .insert_filled(encoded.backdrop_pool_capacity as usize, 0),
            segments: self
                .arenas
                .segments
                .insert_filled(encoded.tile_cnt as usize, 0),
            plan_fingerprint,
            plain_fragment: plan_metadata.plain_fragment,
            local_draw_order: plan_metadata.local_draw_order,
            backdrop_dependencies: backdrop_dependencies(&encoded),
            canvas: encoded,
        };
        Self::add_chunk_resources(
            &mut self.resource_refs,
            &mut self.canvas,
            &chunk.canvas.scene_images,
        );
        Self::remap_chunk_data(&mut self.arenas, &chunk, true);
        self.chunks.insert(id, chunk);
        self.refresh_chunk_dependencies(id);
        NodeRebuild {
            plan_dirty: true,
            transform_only: false,
        }
    }

    pub(crate) fn refresh_chunk_dependencies(&mut self, id: RetainedNodeId) {
        if self
            .chunks
            .get(&id)
            .is_some_and(|chunk| !chunk.backdrop_dependencies.is_empty())
        {
            self.nonlocal_dependencies.insert(id);
        } else {
            self.nonlocal_dependencies.remove(&id);
        }
        if self
            .chunks
            .get(&id)
            .is_some_and(|chunk| chunk_has_surface_dependent_plan(&chunk.canvas))
        {
            self.surface_dependent_plans.insert(id);
        } else {
            self.surface_dependent_plans.remove(&id);
        }
    }

    pub(crate) fn remap_chunk_data(
        arenas: &mut MaterializedArenas,
        chunk: &SceneChunk,
        include_immutable_geometry: bool,
    ) {
        let line_base = if chunk.canvas.lines.is_empty() {
            0
        } else {
            arenas.lines.range(chunk.lines).start as u32
        };
        let path_base = if chunk.canvas.path_records.is_empty() {
            0
        } else {
            arenas.paths.range(chunk.paths).start as u32
        };
        let brush_base = if chunk.canvas.brush_blob.is_empty() {
            0
        } else {
            arenas.brushes.range(chunk.brushes).start as u32
        };
        let sdf_base = if chunk.canvas.sdf_blob.is_empty() {
            0
        } else {
            arenas.sdfs.range(chunk.sdfs).start as u32
        };
        let shadow_base = if chunk.canvas.sdf_shadow_blob.is_empty() {
            0
        } else {
            arenas.shadows.range(chunk.shadows).start as u32
        };
        let glyph_base = chunk.glyphs.map_or(0, |id| {
            arenas.glyphs.as_ref().unwrap().range(id).start as u32
        });
        let run_base = chunk
            .runs
            .filter(|_| !chunk.canvas.text_runs.is_empty())
            .map_or(0, |id| arenas.runs.range(id).start as u32);
        let backdrop_base = if chunk.canvas.backdrop_pool_capacity == 0 {
            0
        } else {
            arenas.backdrops.range(chunk.backdrops).start as u32
        };
        let segment_base = if chunk.canvas.tile_cnt == 0 {
            0
        } else {
            arenas.segments.range(chunk.segments).start as u32
        };

        if include_immutable_geometry && !chunk.canvas.lines.is_empty() {
            arenas
                .lines
                .write_mapped(chunk.lines, &chunk.canvas.lines, |mut line| {
                    line.path_id = line.path_id.saturating_add(path_base);
                    line
                });
        }

        if !chunk.canvas.path_records.is_empty() {
            arenas
                .paths
                .write_mapped(chunk.paths, &chunk.canvas.path_records, |mut path| {
                    path.path_id = path.path_id.saturating_add(path_base);
                    path.line_start = path.line_start.saturating_add(line_base);
                    path.data_offset = path.data_offset.saturating_add(backdrop_base);
                    path.segment_start = path.segment_start.saturating_add(segment_base);
                    path
                });
        }

        if !chunk.canvas.draw_records.is_empty() {
            arenas
                .draws
                .write_mapped(chunk.draws, &chunk.canvas.draw_records, |mut draw| {
                    if draw.path_id != DrawRecord::NONE {
                        draw.path_id = draw.path_id.saturating_add(path_base);
                    }
                    if draw.glyph_run_id != DrawRecord::NONE {
                        draw.glyph_run_id = draw.glyph_run_id.saturating_add(run_base);
                    }
                    if draw.brush_offset != DrawRecord::NONE {
                        draw.brush_offset = draw.brush_offset.saturating_add(brush_base);
                    }
                    if draw.sdf_offset != DrawRecord::NONE {
                        draw.sdf_offset = draw.sdf_offset.saturating_add(sdf_base);
                    }
                    if draw.sdf_shadow_offset != DrawRecord::NONE {
                        draw.sdf_shadow_offset = draw.sdf_shadow_offset.saturating_add(shadow_base);
                    }
                    draw
                });
        }
        if include_immutable_geometry
            && let Some(run_allocation) = chunk.runs.filter(|_| !chunk.canvas.text_runs.is_empty())
        {
            arenas
                .runs
                .write_mapped(run_allocation, &chunk.canvas.text_runs, |run| TextRun {
                    glyph_start: run.glyph_start.saturating_add(glyph_base),
                    glyph_count: run.glyph_count,
                });
        }
    }
}
