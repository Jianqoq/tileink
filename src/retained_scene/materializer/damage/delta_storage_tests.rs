use super::*;
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

fn fixture() -> (RetainedScene, PersistentSceneMaterializer, RetainedNodeId) {
    let root = RetainedNodeId::for_owner(984_000);
    let leaf = RetainedNodeId::for_owner(984_001);
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        Rect::new(4.0, 4.0, 12.0, 12.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let materializer = PersistentSceneMaterializer::new(&scene);
    (scene, materializer, leaf)
}

fn update(
    scene: &RetainedScene,
    materializer: &mut PersistentSceneMaterializer,
) -> crate::canvas::RetainedFrame {
    let changes = scene.changes_since(materializer.version());
    materializer.update(scene, changes);
    materializer.canvas.persistent_frame.clone().unwrap()
}

#[test]
fn invalidation_only_updates_reuse_empty_payloads_and_preserve_version_links() {
    let (mut scene, mut materializer, _) = fixture();
    scene
        .transaction()
        .invalidate_rect(Rect::new(1.0, 1.0, 3.0, 3.0))
        .commit()
        .unwrap();
    let first_frame = update(&scene, &mut materializer);
    let first = first_frame.delta.as_ref().unwrap();
    scene
        .transaction()
        .invalidate_rect(Rect::new(5.0, 5.0, 7.0, 7.0))
        .commit()
        .unwrap();
    let second_frame = update(&scene, &mut materializer);
    let second = second_frame.delta.as_ref().unwrap();
    assert!(!Rc::ptr_eq(first, second));
    assert_eq!(second.from_version, first.to_version);
    assert_eq!(second_frame.version, Some(second.to_version));
    assert_eq!(first_frame.invalidated_bounds, [Bounds::new(1, 1, 3, 3)]);
    assert_eq!(second_frame.invalidated_bounds, [Bounds::new(5, 5, 7, 7)]);
    assert!(
        second.patches.is_empty() && second.damage.is_empty() && second.dirty_backdrops.is_empty()
    );
    // Empty immutable payloads need no new allocation for each version bridge.
    assert!(Rc::ptr_eq(&first.patches, &second.patches));
    assert!(Rc::ptr_eq(&first.damage, &second.damage));
    assert!(Rc::ptr_eq(&first.dirty_backdrops, &second.dirty_backdrops));
    assert!(Rc::ptr_eq(&first.index, &second.index));
}

#[test]
fn affine_updates_reuse_unchanged_index_and_keep_old_patches_immutable() {
    let (mut scene, mut materializer, leaf) = fixture();
    scene
        .transaction()
        .set_transform(leaf, Affine::translate((2.0, 0.0)))
        .commit()
        .unwrap();
    let first_frame = update(&scene, &mut materializer);
    let first = first_frame.delta.as_ref().unwrap();
    let old_bounds = first.patches[0].new.unwrap().bounds;
    scene
        .transaction()
        .set_transform(leaf, Affine::translate((6.0, 0.0)))
        .commit()
        .unwrap();
    let second_frame = update(&scene, &mut materializer);
    let second = second_frame.delta.as_ref().unwrap();
    assert_eq!(first.patches.len(), 1);
    assert_eq!(second.patches.len(), 1);
    assert_eq!(first.patches[0].new.unwrap().bounds, old_bounds);
    assert_ne!(second.patches[0].new.unwrap().bounds, old_bounds);
    assert!(!Rc::ptr_eq(&first.patches, &second.patches));
    // Node IDs and slots are unchanged even though the patch states differ.
    assert!(Rc::ptr_eq(&first.index, &second.index));
    assert!(Rc::ptr_eq(&first.damage, &second.damage));
    assert!(Rc::ptr_eq(&first.dirty_backdrops, &second.dirty_backdrops));
    scene
        .transaction()
        .invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
        .commit()
        .unwrap();
    let third = update(&scene, &mut materializer);
    let third = third.delta.as_ref().unwrap();
    assert!(third.patches.is_empty() && third.index.is_empty());
    scene
        .transaction()
        .set_transform(leaf, Affine::translate((10.0, 0.0)))
        .commit()
        .unwrap();
    let fourth_frame = update(&scene, &mut materializer);
    let fourth = fourth_frame.delta.as_ref().unwrap();
    assert_eq!(fourth.patches.len(), 1);
    assert_eq!(fourth.index.len(), 1);
    assert_eq!(
        fourth.patches[0].new.unwrap().bounds,
        Bounds::new(
            old_bounds.x0 + 8,
            old_bounds.y0,
            old_bounds.x1 + 8,
            old_bounds.y1
        )
    );
    assert!(third.patches.is_empty() && third.index.is_empty());
    assert!(!Rc::ptr_eq(&third.patches, &fourth.patches));
    assert!(!Rc::ptr_eq(&third.index, &fourth.index));
    assert_eq!(first.patches[0].new.unwrap().bounds, old_bounds);
}

#[test]
fn empty_payloads_survive_delete_and_reinsert_without_changing_old_frames() {
    let (mut scene, mut materializer, leaf) = fixture();
    let root = RetainedNodeId::for_owner(984_000);
    let anchor = RetainedNodeId::for_owner(984_002);
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        Rect::new(20.0, 20.0, 28.0, 28.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            anchor,
            Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    update(&scene, &mut materializer);
    scene
        .transaction()
        .invalidate_rect(Rect::new(1.0, 1.0, 3.0, 3.0))
        .commit()
        .unwrap();
    let first = update(&scene, &mut materializer);
    scene
        .transaction()
        .invalidate_rect(Rect::new(5.0, 5.0, 7.0, 7.0))
        .commit()
        .unwrap();
    let second = update(&scene, &mut materializer);
    let empty = second.delta.as_ref().unwrap();
    assert!(Rc::ptr_eq(
        &first.delta.as_ref().unwrap().patches,
        &empty.patches
    ));
    assert!(Rc::ptr_eq(
        &first.delta.as_ref().unwrap().index,
        &empty.index
    ));
    let original = second.node_state(leaf).unwrap().bounds;
    scene.transaction().remove_subtree(leaf).commit().unwrap();
    let removed = update(&scene, &mut materializer);
    assert!(removed.node_state(leaf).is_none());
    assert!(removed.node_state(anchor).is_some());
    let mut replacement = Canvas::new(64, 64, 1.0);
    replacement.push_rect(
        Rect::new(32.0, 8.0, 40.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            Rc::new(replacement),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let reinserted = update(&scene, &mut materializer);
    assert_eq!(
        reinserted.node_state(leaf).unwrap().bounds,
        Bounds::new(32, 8, 40, 16)
    );
    assert!(reinserted.node_state(anchor).is_some());
    // Reusing empty storage must never turn an earlier snapshot into a topology update.
    assert!(empty.patches.is_empty() && empty.index.is_empty());
    assert_eq!(first.node_state(leaf).unwrap().bounds, original);
    assert_eq!(second.node_state(leaf).unwrap().bounds, original);
    assert!(removed.node_state(leaf).is_none());
}

fn patches_with_ids(ids: &[u64]) -> Vec<RetainedNodePatch> {
    let (_, materializer, leaf) = fixture();
    let template = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .node_state(leaf)
        .unwrap();
    ids.iter()
        .map(|&owner| RetainedNodePatch {
            old: None,
            new: Some(crate::canvas::RetainedNodeState {
                id: RetainedNodeId::for_owner(owner),
                ..template
            }),
            damage: None,
        })
        .collect()
}

#[test]
fn multi_node_patch_indexes_rebuild_without_a_reuse_scan() {
    let patches = patches_with_ids(&[81, 82, 83]);
    let original = retained_patch_index(&patches, None);
    for owners in [
        vec![81, 82, 83],
        vec![83, 81, 82],
        vec![81, 82, 84],
        vec![81, 82],
        vec![81, 82, 83, 84],
        vec![81, 81, 82],
        vec![],
    ] {
        let changed = patches_with_ids(&owners);
        let index = retained_patch_index(&changed, Some(&original));
        let expected = owners
            .iter()
            .enumerate()
            .map(|(slot, &owner)| (RetainedNodeId::for_owner(owner), slot))
            .collect::<HashMap<_, _>>();
        assert_eq!(*index, expected);
        assert!(!Rc::ptr_eq(&index, &original));
        // A new layout cannot overwrite an old snapshot's slot lookup.
        for (slot, owner) in [81, 82, 83].into_iter().enumerate() {
            assert_eq!(original.get(&RetainedNodeId::for_owner(owner)), Some(&slot));
        }
    }
    let empty = retained_patch_index(&[], Some(&original));
    assert!(Rc::ptr_eq(&empty, &retained_patch_index(&[], Some(&empty))));
}

#[test]
fn removal_patches_preserve_slots_without_reusing_live_node_states() {
    let mut patches = patches_with_ids(&[91, 92, 93]);
    let original = retained_patch_index(&patches, None);
    for patch in &mut patches {
        patch.old = patch.new.take();
    }
    let removed = retained_patch_index(&patches, Some(&original));
    assert_eq!(*original, *removed);
    for (slot, owner) in [91, 92, 93].into_iter().enumerate() {
        assert_eq!(removed.get(&RetainedNodeId::for_owner(owner)), Some(&slot));
        assert!(patches[slot].new.is_none());
    }
}

#[test]
fn singleton_index_reuse_checks_both_node_identity_and_exact_slot() {
    let first = retained_patch_index(&patches_with_ids(&[101]), None);
    let changed = retained_patch_index(&patches_with_ids(&[102]), Some(&first));
    assert!(!Rc::ptr_eq(&first, &changed));
    assert_eq!(first.get(&RetainedNodeId::for_owner(101)), Some(&0));
    assert!(!first.contains_key(&RetainedNodeId::for_owner(102)));
    assert_eq!(changed.get(&RetainedNodeId::for_owner(102)), Some(&0));
    assert!(!changed.contains_key(&RetainedNodeId::for_owner(101)));

    // A one-key map built from duplicate patches can name a nonzero slot.
    // Cardinality and ID alone must not let that slot escape into a singleton.
    let duplicate = retained_patch_index(&patches_with_ids(&[101, 101]), None);
    assert_eq!(duplicate.get(&RetainedNodeId::for_owner(101)), Some(&1));
    let singleton = retained_patch_index(&patches_with_ids(&[101]), Some(&duplicate));
    assert!(!Rc::ptr_eq(&duplicate, &singleton));
    assert_eq!(singleton.get(&RetainedNodeId::for_owner(101)), Some(&0));
    assert_eq!(duplicate.get(&RetainedNodeId::for_owner(101)), Some(&1));
}
