//! Published node instances own their retirement, revision and hierarchy metadata.
use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn reconcile_scene_metadata(&self, scene: &RetainedScene) -> SceneChangeSet {
        let mut changes = SceneChangeSet {
            invalidate_all: true,
            surface_changed: self.canvas.surface() != scene.surface,
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
            // The journal normally records retirement and insertion independently. Reconcile
            // the same contract after a gap so a live reused ID releases its previous instance.
            if current.instance != previous.instance {
                changes.removed_nodes.insert(id);
            }
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

    #[allow(
        clippy::expect_used,
        reason = "A materialized scene leaf has a parent in the validated retained topology"
    )]
    pub(crate) fn scene_node_metadata(
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> MaterializedNodeMetadata {
        let node = &scene.nodes[&id];
        let order_key = node.parent.map(|parent| {
            scene.nodes[&parent.node]
                .children(parent)
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
}
