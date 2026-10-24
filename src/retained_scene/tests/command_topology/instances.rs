//! Node instance retirement has the same semantics with or without its journal.
use super::*;

#[test]
fn same_id_new_instance_changes_the_materialized_revision() {
    let (mut scene, target) = scene(false);
    let root = RetainedParent::content(scene.root());
    let mut state = PersistentSceneMaterializer::new(&scene);
    let old = state
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .node_revision(target);
    scene
        .transaction()
        .remove_subtree(target)
        .insert_scene(root, None, target, leaf(Color::BLACK), Affine::IDENTITY)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let new = state
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .node_revision(target);
    assert_ne!(old, new);
}

#[test]
fn node_instance_transitions_match_fresh_commands_and_batch_partitions() {
    for gap in [true, false] {
        for before in 0..3 {
            for after in 0..3 {
                let (mut scene, target) = scene(true);
                let root = RetainedParent::content(scene.root());
                let install = |tx: &mut RetainedSceneTransaction<'_>, kind| {
                    tx.remove_subtree(target);
                    match kind {
                        0 => {
                            tx.insert_group(root, None, target);
                        }
                        1 => {
                            tx.insert_scene(
                                root,
                                None,
                                target,
                                leaf(Color::WHITE),
                                Affine::IDENTITY,
                            );
                        }
                        2 => {
                            tx.insert_layer(
                                root,
                                None,
                                target,
                                RetainedLayerDescriptor::ClipPath {
                                    path: Rect::new(0.0, 0.0, 32.0, 32.0).to_path(0.1),
                                    transform: Affine::IDENTITY,
                                    rule: FillRule::NonZero,
                                    tolerance: 0.1,
                                },
                            );
                        }
                        _ => unreachable!(),
                    }
                };
                let mut tx = scene.transaction();
                install(&mut tx, before);
                tx.commit().unwrap();
                let mut state = PersistentSceneMaterializer::new(&scene);
                let mut tx = scene.transaction();
                install(&mut tx, after);
                tx.commit().unwrap();
                if gap {
                    for _ in 0..JOURNAL_CAPACITY {
                        scene.transaction().invalidate_all().commit().unwrap();
                    }
                    assert!(scene.changes_since(state.version()).is_none());
                    assert!(
                        state
                            .reconcile_scene_metadata(&scene)
                            .removed_nodes
                            .contains(&target),
                        "a replaced instance retires even when its ID remains live"
                    );
                }
                assert!(update_materializer(&mut state, &scene));
                let fresh = PersistentSceneMaterializer::new(&scene);
                assert_eq!(state.chunks.len(), fresh.chunks.len());
                assert_eq!(
                    root_retained_commands(&state),
                    root_retained_commands(&fresh),
                    "gap={gap}, before={before}, after={after}"
                );
                let signature = |state: &PersistentSceneMaterializer| {
                    let mut known = Vec::new();
                    let mut ids = scene
                        .nodes
                        .iter()
                        .filter_map(|(&id, node)| {
                            matches!(node.kind, NodeKind::Scene { .. }).then_some(id)
                        })
                        .collect::<Vec<_>>();
                    ids.sort_unstable();
                    ids.into_iter()
                        .map(|id| {
                            state
                                .node_physical_draws(id)
                                .map(|draw| {
                                    let batch = state.canvas.painter.batch_ids[draw];
                                    let group = known
                                        .iter()
                                        .position(|&candidate| candidate == batch)
                                        .unwrap_or_else(|| {
                                            known.push(batch);
                                            known.len() - 1
                                        });
                                    (state.canvas.painter.keys[draw].clone(), group)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    signature(&state),
                    signature(&fresh),
                    "gap={gap}, before={before}, after={after}"
                );
            }
        }
    }
}
