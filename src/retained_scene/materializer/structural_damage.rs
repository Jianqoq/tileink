use super::helpers::chunk_layer_influence_bounds;
use super::*;
use crate::canvas::{RetainedDamage, damage_history::ResolvedDamage};

impl PersistentSceneMaterializer {
    pub(super) fn old_structural_damage(&self, changes: &SceneChangeSet) -> ResolvedDamage {
        let sources = self.structural_damage_sources(changes, |id| {
            self.node_metadata
                .get(&id)
                .map(|node| (node.parent, node.kind == NodeKindTag::Layer))
        });
        // Removed/moved commands still have their old painter position and local
        // dependency domain here. Resolve before chunks or command lists change;
        // feeding their root bounds into the new local tree would apply filters twice.
        let propagated = self.canvas.propagate_damage(&sources);
        ResolvedDamage {
            bounds: propagated.bounds.into_boxed_slice(),
            dirty_backdrops: propagated
                .dirty_backdrops
                .into_iter()
                .collect::<Vec<_>>()
                .into(),
        }
    }

    pub(super) fn new_structural_sources(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> RetainedDamage {
        self.structural_damage_sources(changes, |id| {
            scene
                .nodes
                .get(&id)
                .map(|node| (node.parent, matches!(node.kind, NodeKind::Layer(_))))
        })
    }

    fn structural_damage_sources(
        &self,
        changes: &SceneChangeSet,
        node: impl Fn(RetainedNodeId) -> Option<(Option<RetainedParent>, bool)>,
    ) -> RetainedDamage {
        let mut damage = RetainedDamage::default();
        for (&id, chunk) in &self.chunks {
            let Some((_, layer)) = node(id) else { continue };
            let mut cursor = Some(id);
            while let Some(current) = cursor {
                if changes.changed_nodes.contains(&current)
                    || changes.removed_nodes.contains(&current)
                {
                    // Groups have no command/chunk of their own. Their changed
                    // placement affects each descendant's actual local output.
                    damage.add_node(
                        id,
                        if layer {
                            chunk_layer_influence_bounds(chunk)
                        } else {
                            chunk.canvas.visual_bounds()
                        },
                    );
                    break;
                }
                cursor = node(current).and_then(|(parent, _)| parent.map(|parent| parent.node));
            }
        }
        damage
    }
}

#[cfg(test)]
mod tests;
