use super::*;

impl PersistentSceneMaterializer {
    #[allow(
        clippy::expect_used,
        reason = "Spatial rebuilding follows installation of the persistent frame"
    )]
    pub(crate) fn rebuild_spatial_index(&mut self, scene: &RetainedScene) {
        self.node_bounds.clear();
        self.raw_node_bounds.clear();
        self.bounded_node_bounds.clear();
        self.bounded_raw_node_bounds.clear();
        let tiles_size = (
            Rc::make_mut(&mut self.canvas).width_in_tiles(),
            Rc::make_mut(&mut self.canvas).height_in_tiles(),
        );
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        self.node_tiles.clear();
        self.node_tiles.resize(tile_count, HashSet::default());
        self.spatial_nodes.clear();
        self.raw_node_tiles.clear();
        self.raw_node_tiles.resize(tile_count, HashSet::default());
        self.spatial_tiles_size = tiles_size;
        let frame = Rc::make_mut(&mut self.canvas)
            .persistent_frame
            .clone()
            .expect("persistent frame override");
        for &node in frame.nodes.iter() {
            self.set_node_bounds_spatial(
                node.id,
                Some(node.bounds),
                self.fixed_translation_bounds(scene, node.id),
            );
        }
        let raw_bounds = self
            .chunks
            .iter()
            .filter_map(|(&id, chunk)| {
                // Removed chunks are reclaimed later in the update after their old plan metadata
                // has been inspected. A resize can rebuild this index first, so derive it only
                // from nodes that are live in the new scene. This fixes the update ordering at
                // its source instead of special-casing resize transactions in callers.
                let node = scene.nodes.get(&id)?;
                let bounds = match node.kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
                    NodeKind::Group => unreachable!("groups do not own chunks"),
                };
                Some((id, bounds))
            })
            .collect::<Vec<_>>();
        for (id, bounds) in raw_bounds {
            self.set_raw_node_bounds_spatial(
                id,
                Some(bounds),
                self.fixed_translation_bounds(scene, id),
            );
        }
    }

    /// Refreshes only changed spatial entries after a non-structural frame-state fallback.
    ///
    /// Rebuilding `RetainedFrame` can be cheaper than proving a dependency-aware delta, but that
    /// does not invalidate the unchanged nodes' tile memberships. Reinitializing every tile
    /// `HashSet` here made a full-screen bounded chart translation pay O(surface tiles) each frame.
    pub(crate) fn patch_changed_spatial_index(
        &mut self,
        scene: &RetainedScene,
        changed: &HashSet<RetainedNodeId>,
        removed: &HashSet<RetainedNodeId>,
    ) -> bool {
        if self.spatial_tiles_size != (self.canvas.width_in_tiles(), self.canvas.height_in_tiles())
        {
            return false;
        }
        let Some(frame) = self.canvas.persistent_frame.clone() else {
            return false;
        };
        for &id in removed {
            self.set_node_bounds_spatial(id, None, None);
            self.set_raw_node_bounds_spatial(id, None, None);
        }
        for &id in changed {
            let Some(node) = scene.nodes.get(&id) else {
                return false;
            };
            if matches!(node.kind, NodeKind::Group) {
                continue;
            }
            let Some(state) = frame.node_state(id) else {
                return false;
            };
            let Some(chunk) = self.chunks.get(&id) else {
                return false;
            };
            let fixed = self.fixed_translation_bounds(scene, id);
            let raw = if let Some(fixed) = fixed {
                self.raw_node_bounds.get(&id).copied().unwrap_or(fixed)
            } else {
                match node.kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
                    NodeKind::Group => unreachable!("groups do not own chunks"),
                }
            };
            self.set_node_bounds_spatial(id, Some(state.bounds), fixed);
            self.set_raw_node_bounds_spatial(id, Some(raw), fixed);
        }
        true
    }

    pub(crate) fn set_node_bounds_spatial(
        &mut self,
        id: RetainedNodeId,
        bounds: Option<Bounds>,
        fixed_spatial_bounds: Option<Bounds>,
    ) {
        let bounds = bounds.filter(|bounds| !bounds.is_empty());
        let fixed_spatial_bounds = fixed_spatial_bounds.filter(|bounds| !bounds.is_empty());
        let was_spatial = self.spatial_nodes.contains(&id);
        let should_be_spatial = bounds.is_some() && self.painter_bases.contains_key(&id);
        if self.node_bounds.get(&id).copied() == bounds
            && self.bounded_node_bounds.get(&id).copied() == fixed_spatial_bounds
            && was_spatial == should_be_spatial
        {
            return;
        }
        let old_fixed = self.bounded_node_bounds.remove(&id);
        let old = match self.node_bounds.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = *entry.get();
                if Some(old) != bounds {
                    if let Some(bounds) = bounds {
                        *entry.get_mut() = bounds;
                    } else {
                        entry.remove();
                    }
                }
                Some(old)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                if let Some(bounds) = bounds {
                    entry.insert(bounds);
                }
                None
            }
        };
        if was_spatial
            && old_fixed.is_none()
            && let Some(old) = old
        {
            for tile in self.tiles_for_bounds(old) {
                self.node_tiles[tile].remove(&id);
            }
        }
        let is_spatial = should_be_spatial;
        if is_spatial && let Some(bounds) = bounds {
            if let Some(fixed) = fixed_spatial_bounds {
                self.bounded_node_bounds.insert(id, fixed);
            } else {
                for tile in self.tiles_for_bounds(bounds) {
                    self.node_tiles[tile].insert(id);
                }
            }
            self.spatial_nodes.insert(id);
        } else {
            self.spatial_nodes.remove(&id);
        }
    }

    pub(crate) fn set_raw_node_bounds_spatial(
        &mut self,
        id: RetainedNodeId,
        bounds: Option<Bounds>,
        fixed_spatial_bounds: Option<Bounds>,
    ) {
        let bounds = bounds.filter(|bounds| !bounds.is_empty());
        let fixed_spatial_bounds = fixed_spatial_bounds.filter(|bounds| !bounds.is_empty());
        if self.raw_node_bounds.get(&id).copied() == bounds
            && self.bounded_raw_node_bounds.get(&id).copied() == fixed_spatial_bounds
        {
            return;
        }
        let old_fixed = self.bounded_raw_node_bounds.remove(&id);
        let old = match self.raw_node_bounds.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = *entry.get();
                if Some(old) != bounds {
                    if let Some(bounds) = bounds {
                        *entry.get_mut() = bounds;
                    } else {
                        entry.remove();
                    }
                }
                Some(old)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                if let Some(bounds) = bounds {
                    entry.insert(bounds);
                }
                None
            }
        };
        if old_fixed.is_none()
            && let Some(old) = old
        {
            for tile in self.tiles_for_bounds(old) {
                self.raw_node_tiles[tile].remove(&id);
            }
        }
        if let Some(bounds) = bounds {
            if let Some(fixed) = fixed_spatial_bounds {
                self.bounded_raw_node_bounds.insert(id, fixed);
            } else {
                for tile in self.tiles_for_bounds(bounds) {
                    self.raw_node_tiles[tile].insert(id);
                }
            }
        }
    }

    pub(crate) fn tiles_for_bounds(
        &self,
        bounds: Bounds,
    ) -> super::super::spatial_tiles::BoundsTileIter {
        super::super::spatial_tiles::BoundsTileIter::new(
            bounds,
            self.canvas.width_in_tiles(),
            self.canvas.height_in_tiles(),
        )
    }

    pub(crate) fn spatial_candidates(&self, bounds: Bounds) -> HashSet<RetainedNodeId> {
        let mut candidates = HashSet::default();
        for tile in self.tiles_for_bounds(bounds) {
            candidates.extend(self.node_tiles[tile].iter().copied());
        }
        candidates.extend(
            self.bounded_node_bounds
                .iter()
                .filter_map(|(&id, &fixed)| (!fixed.intersect(bounds).is_empty()).then_some(id)),
        );
        candidates
    }

    pub(crate) fn raw_spatial_candidates(&self, bounds: Bounds) -> HashSet<RetainedNodeId> {
        let mut candidates = HashSet::default();
        for tile in self.tiles_for_bounds(bounds) {
            candidates.extend(self.raw_node_tiles[tile].iter().copied());
        }
        candidates.extend(
            self.bounded_raw_node_bounds
                .iter()
                .filter_map(|(&id, &fixed)| (!fixed.intersect(bounds).is_empty()).then_some(id)),
        );
        candidates
    }

    pub(crate) fn root_reorder_damage(
        &self,
        old_bases: &HashMap<RetainedNodeId, Rc<[u128]>>,
        changed: &HashSet<RetainedNodeId>,
    ) -> Vec<(RetainedNodeId, Bounds)> {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        for &id in changed {
            let (Some(&bounds), Some(old), Some(new)) = (
                self.node_bounds.get(&id),
                old_bases.get(&id),
                self.painter_bases.get(&id),
            ) else {
                continue;
            };
            for candidate in self.spatial_candidates(bounds) {
                if candidate == id || !self.node_bounds.contains_key(&candidate) {
                    continue;
                }
                let Some(candidate_new) = self.painter_bases.get(&candidate) else {
                    continue;
                };
                let candidate_old = old_bases.get(&candidate).unwrap_or(candidate_new);
                if (old.as_ref() < candidate_old.as_ref())
                    == (new.as_ref() < candidate_new.as_ref())
                {
                    continue;
                }
                let overlap = bounds.intersect(self.node_bounds[&candidate]);
                if !overlap.is_empty() {
                    damage
                        .entry(id)
                        .and_modify(|current| *current = current.union(overlap))
                        .or_insert(overlap);
                }
            }
        }
        damage.into_iter().collect()
    }
}
