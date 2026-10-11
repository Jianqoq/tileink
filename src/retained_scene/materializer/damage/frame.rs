use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn rebuild_frame_override(&mut self, scene: &RetainedScene) {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        let mut influences = HashMap::default();
        self.collect_bounds_influences(
            scene,
            scene.root,
            BoundsInfluence {
                outset: 0,
                clip: Some(canvas_bounds),
            },
            &mut influences,
        );
        let leaf_bounds = scene
            .nodes
            .iter()
            .filter(|(_, node)| matches!(node.kind, NodeKind::Scene { .. }))
            .map(|(&id, _)| {
                let chunk = &self.chunks[&id];
                let natural = influences[&id].apply(chunk.canvas.visual_bounds());
                (id, self.retained_output_bounds(scene, id, natural))
            })
            .collect::<HashMap<_, _>>();
        let mut subtree_leaf_bounds = HashMap::default();
        self.collect_subtree_leaf_bounds(scene, scene.root, &leaf_bounds, &mut subtree_leaf_bounds);
        let mut nodes = Vec::with_capacity(self.chunks.len());
        self.collect_frame_nodes(
            scene,
            RetainedParent::content(scene.root),
            &leaf_bounds,
            &subtree_leaf_bounds,
            &influences,
            &mut nodes,
        );
        let node_index = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        let scoped_damage = self.scoped_backdrop;
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.persistent_frame = Some(crate::canvas::RetainedFrame {
            root: scene.root,
            logical_size: (scene.width, scene.height),
            physical_size: canvas.physical_size(),
            scale_bits: scene.scale.to_bits(),
            nodes: nodes.into(),
            node_index: Rc::new(node_index),
            state_pages: Rc::new(HashMap::default()),
            invalidated_bounds: canvas.invalidated_bounds.clone(),
            invalidate_all: canvas.invalidate_all,
            version: scene.version.get(),
            delta: None,
            damage_history: crate::canvas::damage_history::DamageHistory::initial(
                scoped_damage,
                scene.version.get(),
            ),
            requires_damage_propagation: !self.nonlocal_dependencies.is_empty(),
        });
    }

    pub(crate) fn collect_frame_nodes(
        &self,
        scene: &RetainedScene,
        parent: RetainedParent,
        leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        subtree_leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        influences: &HashMap<RetainedNodeId, BoundsInfluence>,
        nodes: &mut Vec<crate::canvas::RetainedNodeState>,
    ) {
        let children = scene.nodes[&parent.node]
            .children(parent)
            .expect("validated retained branch");
        for &id in children.values() {
            let node = &scene.nodes[&id];
            match &node.kind {
                NodeKind::Group => {
                    self.collect_frame_nodes(
                        scene,
                        RetainedParent::content(id),
                        leaf_bounds,
                        subtree_leaf_bounds,
                        influences,
                        nodes,
                    );
                }
                NodeKind::Scene { .. } => {
                    nodes.push(crate::canvas::RetainedNodeState {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        bounds: leaf_bounds[&id],
                        kind: RetainedNodeKind::Scene,
                    });
                }
                NodeKind::Layer(descriptor) => {
                    self.collect_frame_nodes(
                        scene,
                        RetainedParent::content(id),
                        leaf_bounds,
                        subtree_leaf_bounds,
                        influences,
                        nodes,
                    );
                    if matches!(descriptor, RetainedLayerDescriptor::Mask(_)) {
                        self.collect_frame_nodes(
                            scene,
                            RetainedParent::mask(id),
                            leaf_bounds,
                            subtree_leaf_bounds,
                            influences,
                            nodes,
                        );
                    }
                    let chunk = &self.chunks[&id];
                    let bounds = influences[&id]
                        .apply(chunk_layer_influence_bounds(chunk))
                        .union(subtree_leaf_bounds[&id]);
                    nodes.push(crate::canvas::RetainedNodeState {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        bounds,
                        kind: RetainedNodeKind::Layer,
                    });
                }
            }
        }
    }

    pub(crate) fn collect_bounds_influences(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        inherited: BoundsInfluence,
        influences: &mut HashMap<RetainedNodeId, BoundsInfluence>,
    ) {
        influences.insert(id, inherited);
        let node = &scene.nodes[&id];
        let child_influence = if let NodeKind::Layer(layer) = &node.kind {
            inherited.with_nearer(self.local_bounds_influence(id, layer))
        } else {
            inherited
        };
        for child in node.content.values().chain(node.mask.values()) {
            self.collect_bounds_influences(scene, *child, child_influence, influences);
        }
    }

    pub(crate) fn local_bounds_influence(
        &self,
        id: RetainedNodeId,
        layer: &RetainedLayerDescriptor,
    ) -> BoundsInfluence {
        match layer {
            RetainedLayerDescriptor::ClipPath { .. }
            | RetainedLayerDescriptor::ClipSdf { .. }
            | RetainedLayerDescriptor::Isolate { .. }
            | RetainedLayerDescriptor::Opacity { .. }
            | RetainedLayerDescriptor::Blend { .. } => BoundsInfluence {
                outset: 0,
                clip: self.chunks[&id].canvas.draw_records.first().map(|draw| {
                    let clip = draw.pixel_bounds;
                    Bounds::new(clip.x0, clip.y0, clip.x1, clip.y1)
                }),
            },
            RetainedLayerDescriptor::Filter { .. } => {
                let (value, sample_region) = chunk_filter(&self.chunks[&id]);
                let input = filter::filter_input_bounds(
                    value,
                    sample_region,
                    Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height()),
                );
                let output = filter::unclipped_filtered_region_bounds(value, sample_region);
                let outset = filter::filter_dependency(value).damage_outset(input.union(output));
                BoundsInfluence {
                    outset,
                    // A conservative finite summary preserves read influence through
                    // nested clips; filter output geometry itself is unchanged.
                    clip: Some(input.outset(outset).intersect(output)),
                }
            }
            RetainedLayerDescriptor::Mask(_) => BoundsInfluence {
                outset: 0,
                clip: Some(chunk_layer_influence_bounds(&self.chunks[&id])),
            },
            RetainedLayerDescriptor::Backdrop { .. } => BoundsInfluence {
                outset: 0,
                clip: None,
            },
        }
    }

    pub(crate) fn collect_subtree_leaf_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        subtree: &mut HashMap<RetainedNodeId, Bounds>,
    ) -> Bounds {
        let node = &scene.nodes[&id];
        let bounds = match &node.kind {
            NodeKind::Scene { .. } => leaf_bounds[&id],
            NodeKind::Group | NodeKind::Layer(_) => node
                .content
                .values()
                .chain(node.mask.values())
                .copied()
                .map(|child| self.collect_subtree_leaf_bounds(scene, child, leaf_bounds, subtree))
                .fold(Bounds::new(0, 0, 0, 0), Bounds::union),
        };
        subtree.insert(id, bounds);
        bounds
    }
}
