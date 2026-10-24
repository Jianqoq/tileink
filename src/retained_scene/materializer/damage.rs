//! Dependency propagation, with frame construction, patches and spatial indexes owned separately.
use super::helpers::*;
use super::*;

mod frame;
mod patch_content;
mod patch_local;
mod patch_surface;
mod patch_topology;
mod spatial;

impl PersistentSceneMaterializer {
    /// Painter order changes only with the dependency set or hierarchy. Keeping
    /// this index avoids allocating paths and sorting every unchanged backdrop
    /// on ordinary revisions; propagation still visits every dependency.
    pub(super) fn rebuild_backdrop_order(&mut self, scene: &RetainedScene) {
        self.backdrop_order.clear();
        if self.scoped_backdrop || self.nonlocal_dependencies.is_empty() {
            return;
        }
        #[cfg(test)]
        self.backdrop_order_sorts
            .set(self.backdrop_order_sorts.get() + 1);
        self.backdrop_order.extend(
            self.nonlocal_dependencies
                .iter()
                .map(|&id| (id, painter_path(scene, id))),
        );
        self.backdrop_order
            .sort_unstable_by(|left, right| left.1.as_ref().cmp(right.1.as_ref()));
    }

    pub(crate) fn influenced_bounds(
        &self,
        scene: &RetainedScene,
        mut id: RetainedNodeId,
        mut bounds: Bounds,
    ) -> Bounds {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        while let Some(parent) = scene.nodes[&id].parent {
            let parent_node = &scene.nodes[&parent.node];
            if let NodeKind::Layer(layer) = &parent_node.kind {
                bounds = match layer {
                    RetainedLayerDescriptor::ClipPath { .. }
                    | RetainedLayerDescriptor::ClipSdf { .. }
                    | RetainedLayerDescriptor::Isolate { .. }
                    | RetainedLayerDescriptor::Opacity { .. }
                    | RetainedLayerDescriptor::Blend { .. } => self.chunks[&parent.node]
                        .canvas
                        .draw_records
                        .first()
                        .map_or(bounds, |draw| {
                            let clip = draw.pixel_bounds;
                            bounds.intersect(Bounds::new(clip.x0, clip.y0, clip.x1, clip.y1))
                        }),
                    RetainedLayerDescriptor::Filter { .. } => {
                        let (value, sample_region) = chunk_filter(&self.chunks[&parent.node]);
                        filter::filter_dependency(value).affected_output(
                            bounds,
                            filter::filter_input_bounds(value, sample_region, canvas_bounds),
                            filter::unclipped_filtered_region_bounds(value, sample_region),
                        )
                    }
                    RetainedLayerDescriptor::Mask(_) => {
                        bounds.intersect(chunk_layer_influence_bounds(&self.chunks[&parent.node]))
                    }
                    RetainedLayerDescriptor::Backdrop { .. } => bounds,
                };
            }
            id = parent.node;
        }
        // Intermediate outputs may lie outside the root and be read by an
        // outer filter. Only the complete ancestor influence is root-clipped.
        bounds.intersect(canvas_bounds)
    }

    pub(crate) fn fixed_translation_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> Option<Bounds> {
        self.fixed_translation_bounds_for_node(scene, scene.nodes.get(&id)?)
    }

    fn fixed_translation_bounds_for_node(
        &self,
        scene: &RetainedScene,
        node: &SceneNode,
    ) -> Option<Bounds> {
        let NodeKind::Scene {
            translation_damage: Some(rect),
            ..
        } = &node.kind
        else {
            return None;
        };
        let scale = f64::from(scene.surface.scale_factor());
        Some(
            Bounds::new(
                (rect.x0 * scale).floor() as i32,
                (rect.y0 * scale).floor() as i32,
                (rect.x1 * scale).ceil() as i32,
                (rect.y1 * scale).ceil() as i32,
            )
            .intersect(Bounds::canvas(
                self.canvas.physical_width(),
                self.canvas.physical_height(),
            )),
        )
    }

    pub(crate) fn retained_output_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        natural: Bounds,
    ) -> Bounds {
        self.fixed_translation_bounds(scene, id).unwrap_or(natural)
    }

    /// Resolves backdrop dependencies from the persistent hierarchy and painter index. Frame
    /// node bounds already include ordinary filter influence, while a backdrop additionally
    /// depends on earlier siblings intersecting its sample region.
    pub(crate) fn incremental_backdrop_damage(
        &self,
        scene: &RetainedScene,
        sources: &[(Option<RetainedNodeId>, Bounds)],
    ) -> (Vec<(RetainedNodeId, Bounds)>, Vec<RetainedNodeId>) {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        let mut dirty = Vec::new();
        let mut source_nodes = HashSet::default();
        let mut ordered_sources = Vec::new();
        let mut damage_tiles = HashMap::default();
        for &(source, bounds) in sources {
            let Some(source) = source else {
                self.add_indexed_damage(&mut damage_tiles, bounds);
                continue;
            };
            source_nodes.insert(source);
            let Some(painter) = self.painter_bases.get(&source).cloned().or_else(|| {
                scene
                    .nodes
                    .contains_key(&source)
                    .then(|| painter_path(scene, source))
            }) else {
                continue;
            };
            ordered_sources.push((painter, bounds));
        }
        ordered_sources.sort_unstable_by(|left, right| left.0.as_ref().cmp(right.0.as_ref()));
        let mut source_cursor = 0;
        for (backdrop, backdrop_painter) in &self.backdrop_order {
            let backdrop = *backdrop;
            let Some(chunk) = self.chunks.get(&backdrop) else {
                continue;
            };
            while source_cursor < ordered_sources.len()
                && ordered_sources[source_cursor].0.as_ref() < backdrop_painter.as_ref()
            {
                self.add_indexed_damage(&mut damage_tiles, ordered_sources[source_cursor].1);
                source_cursor += 1;
            }
            let mut affected_output = if source_nodes.contains(&backdrop) {
                self.node_bounds.get(&backdrop).copied().unwrap_or_else(|| {
                    chunk
                        .backdrop_dependencies
                        .iter()
                        .fold(Bounds::new(0, 0, 0, 0), |bounds, dependency| {
                            bounds.union(dependency.output)
                        })
                })
            } else {
                Bounds::new(0, 0, 0, 0)
            };
            for dependency in &chunk.backdrop_dependencies {
                if source_nodes.contains(&backdrop) {
                    // Replacing a backdrop discards its unfiltered history. Replay
                    // its entire read footprint in painter order: clean halo tiles
                    // may still contain later foreground from the previous frame.
                    affected_output = affected_output.union(dependency.dependency);
                }
                let sampled =
                    self.indexed_damage_intersection(&damage_tiles, dependency.dependency);
                if !sampled.is_empty() {
                    affected_output = affected_output.union(dependency.read.affected_output(
                        sampled,
                        dependency.dependency,
                        dependency.output,
                    ));
                }
            }
            if !affected_output.is_empty() {
                dirty.push(backdrop);
                // A changed earlier backdrop becomes sampled background for later backdrops. Tile
                // unions keep cascade propagation proportional to affected pixels instead of the
                // number of all earlier backdrop/source pairs.
                self.add_indexed_damage(&mut damage_tiles, affected_output);
                damage
                    .entry(backdrop)
                    .and_modify(|current| *current = current.union(affected_output))
                    .or_insert(affected_output);
            }
        }
        (damage.into_iter().collect(), dirty)
    }

    pub(crate) fn add_indexed_damage(&self, tiles: &mut HashMap<usize, Bounds>, bounds: Bounds) {
        for tile in self.tiles_for_bounds(bounds) {
            tiles
                .entry(tile)
                .and_modify(|current| *current = current.union(bounds))
                .or_insert(bounds);
        }
    }

    pub(crate) fn indexed_damage_intersection(
        &self,
        tiles: &HashMap<usize, Bounds>,
        bounds: Bounds,
    ) -> Bounds {
        self.tiles_for_bounds(bounds)
            .filter_map(|tile| tiles.get(&tile).copied())
            .map(|damage| damage.intersect(bounds))
            .fold(Bounds::new(0, 0, 0, 0), Bounds::union)
    }
}

#[cfg(test)]
mod filter_dependency_tests;

#[cfg(test)]
mod backdrop_tests;

#[cfg(test)]
mod scoped_history_tests;

#[cfg(test)]
mod isolation_tests;

#[cfg(test)]
mod order_tests;

#[cfg(test)]
mod delta_storage_tests;
