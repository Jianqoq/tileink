use super::*;
use crate::{Filter, Radius, Region, RetainedLayerDescriptor, RetainedParent};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

const ROOT: RetainedNodeId = RetainedNodeId::new(1, 0);
const SOURCE: RetainedNodeId = RetainedNodeId::new(2, 0);
const LEAF: RetainedNodeId = RetainedNodeId::new(3, 0);
const BACKDROP: RetainedNodeId = RetainedNodeId::new(10, 0);

fn layer(backdrop: bool, amount: f32) -> RetainedLayerDescriptor {
    let sample_region = Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO);
    let filter = Filter::Invert(amount);
    if backdrop {
        RetainedLayerDescriptor::Backdrop {
            filter,
            sample_region,
        }
    } else {
        RetainedLayerDescriptor::Filter {
            filter,
            sample_region,
        }
    }
}

fn content(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    Rc::new(canvas)
}

fn fixture(scoped: bool, count: u64) -> (RetainedScene, PersistentSceneMaterializer) {
    let mut scene = RetainedScene::new(64, 64, 1.0, ROOT).unwrap();
    let mut tx = scene.transaction();
    tx.insert_layer(
        RetainedParent::content(ROOT),
        None,
        SOURCE,
        layer(false, 0.0),
    );
    tx.insert_scene(
        RetainedParent::content(SOURCE),
        None,
        LEAF,
        content(Color::from_rgb8(0, 0, 255)),
        Affine::IDENTITY,
    );
    for index in 0..count {
        tx.insert_layer(
            RetainedParent::content(if scoped { SOURCE } else { ROOT }),
            None,
            RetainedNodeId::new(index + 10, 0),
            layer(true, 1.0),
        );
    }
    tx.commit().unwrap();
    let materializer = PersistentSceneMaterializer::new(&scene);
    (scene, materializer)
}

fn update(scene: &RetainedScene, materializer: &mut PersistentSceneMaterializer, gap: bool) {
    let changes = (!gap)
        .then(|| scene.changes_since(materializer.version()))
        .flatten();
    materializer.update(scene, changes);
    assert_eq!(materializer.version(), scene.version);
    let rebuilt = PersistentSceneMaterializer::new(scene);
    assert_eq!(materializer.scoped_backdrop, rebuilt.scoped_backdrop);
    assert_eq!(materializer.backdrop_order, rebuilt.backdrop_order);
    assert_eq!(
        materializer.nonlocal_dependencies,
        rebuilt.nonlocal_dependencies
    );
    assert_eq!(
        materializer.surface_dependent_plans,
        rebuilt.surface_dependent_plans
    );
}

#[test]
fn ordinary_revisions_reuse_scoped_backdrop_classification() {
    for scoped in [false, true] {
        for count in [0, 1, 16] {
            let (mut scene, mut materializer) = fixture(scoped, count);
            let scans = materializer.scoped_backdrop_scans.get();
            for amount in [1.0, 0.0, 0.5] {
                scene
                    .transaction()
                    .update_layer(SOURCE, layer(false, amount))
                    .commit()
                    .unwrap();
                update(&scene, &mut materializer, false);
                assert_eq!(materializer.scoped_backdrop, scoped && count != 0);
                // A parameter change cannot change which backdrop belongs to an input domain.
                // Rechecking every unchanged dependency was the O(backdrops) regression.
                assert_eq!(materializer.scoped_backdrop_scans.get(), scans);
            }
            scene
                .transaction()
                .replace_scene(LEAF, content(Color::from_rgb8(255, 0, 0)))
                .commit()
                .unwrap();
            update(&scene, &mut materializer, false);
            assert_eq!(materializer.scoped_backdrop_scans.get(), scans);
            scene
                .transaction()
                .invalidate_rect(Rect::new(0.0, 0.0, 4.0, 4.0))
                .commit()
                .unwrap();
            update(&scene, &mut materializer, false);
            assert_eq!(materializer.scoped_backdrop_scans.get(), scans);
        }
    }
}

#[test]
fn dependency_edits_refresh_scoped_backdrop_classification() {
    for gap in [false, true] {
        let (mut scene, mut materializer) = fixture(false, 1);
        assert!(!materializer.scoped_backdrop);
        scene
            .transaction()
            .reparent(BACKDROP, RetainedParent::content(SOURCE), None)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(materializer.scoped_backdrop);
        scene
            .transaction()
            .update_layer(BACKDROP, layer(false, 0.5))
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(
            !materializer.scoped_backdrop,
            "removing the old dependency invalidates its cached domain"
        );
        scene
            .transaction()
            .update_layer(BACKDROP, layer(true, 1.0))
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(
            materializer.scoped_backdrop,
            "a newly added dependency invalidates the cached domain"
        );
        scene
            .transaction()
            .reparent(BACKDROP, RetainedParent::content(ROOT), None)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(!materializer.scoped_backdrop);
        scene
            .transaction()
            .reparent(BACKDROP, RetainedParent::content(SOURCE), None)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(materializer.scoped_backdrop);
        scene
            .transaction()
            .remove_subtree(BACKDROP)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(!materializer.scoped_backdrop);
    }
}

#[test]
fn moving_only_a_dependency_ancestor_refreshes_scoped_classification() {
    let (mut scene, mut materializer) = fixture(false, 1);
    let group = RetainedNodeId::new(30, 0);
    scene
        .transaction()
        .insert_group(RetainedParent::content(ROOT), None, group)
        .reparent(BACKDROP, RetainedParent::content(group), None)
        .commit()
        .unwrap();
    update(&scene, &mut materializer, false);
    assert!(!materializer.scoped_backdrop);
    assert!(materializer.nonlocal_dependencies.contains(&BACKDROP));
    scene
        .transaction()
        .reparent(group, RetainedParent::content(SOURCE), None)
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version()).unwrap();
    assert!(changes.hierarchy_changed);
    assert!(!changes.changed_nodes.contains(&BACKDROP));
    update(&scene, &mut materializer, false);
    assert!(materializer.scoped_backdrop);
    scene
        .transaction()
        .reparent(group, RetainedParent::content(ROOT), None)
        .commit()
        .unwrap();
    update(&scene, &mut materializer, false);
    assert!(!materializer.scoped_backdrop);
}

#[test]
fn same_scale_resize_refreshes_classification_then_resumes_reuse() {
    for scoped in [false, true] {
        let (mut scene, mut materializer) = fixture(scoped, 1);
        for (width, height, amount) in [(80, 64, 1.0), (96, 80, 0.0)] {
            scene
                .transaction()
                .resize(width, height, 1.0)
                .commit()
                .unwrap();
            update(&scene, &mut materializer, false);
            assert_eq!(materializer.scoped_backdrop, scoped);
            let scans = materializer.scoped_backdrop_scans.get();
            scene
                .transaction()
                .update_layer(SOURCE, layer(false, amount))
                .commit()
                .unwrap();
            update(&scene, &mut materializer, false);
            assert_eq!(materializer.scoped_backdrop, scoped);
            assert_eq!(materializer.scoped_backdrop_scans.get(), scans);
        }
    }
}

fn backdrop_scene(nested: bool) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    let rect = Rect::new(0.0, 0.0, 8.0, 8.0);
    let region = || Region::rect(rect, Radius::ZERO);
    if nested {
        canvas.push_filter_layer(Filter::Invert(0.0), region());
    }
    canvas.push_rect(rect, Radius::ZERO, Color::from_rgb8(0, 0, 255));
    canvas.push_backdrop_layer(Filter::Invert(1.0), region());
    canvas.pop_layer();
    if nested {
        canvas.pop_layer();
    }
    Rc::new(canvas)
}

#[test]
fn scene_replacement_refreshes_root_and_nested_dependency_domains() {
    let (mut scene, _) = fixture(false, 0);
    scene
        .transaction()
        .reparent(LEAF, RetainedParent::content(ROOT), None)
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    for (canvas, scoped) in [
        (content(Color::from_rgb8(0, 0, 255)), false),
        (backdrop_scene(false), false),
        (backdrop_scene(true), true),
        (backdrop_scene(false), false),
        (content(Color::from_rgb8(255, 0, 0)), false),
    ] {
        scene
            .transaction()
            .replace_scene(LEAF, canvas)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, false);
        assert_eq!(materializer.scoped_backdrop, scoped);
    }
}

#[test]
fn reparenting_filter_only_layer_preserves_surface_dependency() {
    let (mut scene, mut materializer) = fixture(false, 0);
    assert!(materializer.nonlocal_dependencies.is_empty());
    assert!(materializer.surface_dependent_plans.contains(&SOURCE));
    let group = RetainedNodeId::new(40, 0);
    scene
        .transaction()
        .insert_group(RetainedParent::content(ROOT), None, group)
        .reparent(SOURCE, RetainedParent::content(group), None)
        .commit()
        .unwrap();
    update(&scene, &mut materializer, false);
    assert!(materializer.nonlocal_dependencies.is_empty());
    assert!(materializer.surface_dependent_plans.contains(&SOURCE));
}

#[test]
fn ordinary_root_backdrop_updates_reuse_painter_order() {
    for count in [0, 1, 16] {
        let (mut scene, mut materializer) = fixture(false, count);
        let sorts = materializer.backdrop_order_sorts.get();
        for amount in [1.0, 0.0, 0.5] {
            scene
                .transaction()
                .update_layer(SOURCE, layer(false, amount))
                .commit()
                .unwrap();
            update(&scene, &mut materializer, false);
            // Parameter changes preserve dependency painter order. Sorting all
            // unchanged backdrops dominated the measured root-update CPU cost.
            assert_eq!(materializer.backdrop_order_sorts.get(), sorts);
        }
    }
}

#[test]
fn root_dependency_order_tracks_moves_removal_and_journal_recovery() {
    for gap in [false, true] {
        let (mut scene, mut materializer) = fixture(false, 2);
        let second = RetainedNodeId::new(11, 0);
        scene
            .transaction()
            .move_before(second, BACKDROP)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert_eq!(
            materializer
                .backdrop_order
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [second, BACKDROP]
        );
        scene
            .transaction()
            .reparent(second, RetainedParent::content(SOURCE), None)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert!(materializer.scoped_backdrop && materializer.backdrop_order.is_empty());
        scene
            .transaction()
            .reparent(second, RetainedParent::content(ROOT), Some(BACKDROP))
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert_eq!(
            materializer
                .backdrop_order
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [second, BACKDROP]
        );
        scene
            .transaction()
            .remove_subtree(BACKDROP)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, gap);
        assert_eq!(
            materializer
                .backdrop_order
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [second]
        );
    }
}

#[test]
fn scoped_layer_patch_reuses_already_computed_local_bounds() {
    let (mut scene, mut materializer) = fixture(true, 1);
    for amount in [1.0, 0.0, 0.5] {
        let reads = materializer.local_output_bounds_reads.get();
        scene
            .transaction()
            .update_layer(SOURCE, layer(false, amount))
            .commit()
            .unwrap();
        update(&scene, &mut materializer, false);
        // The old layer domain is already known to patching. Compute only the
        // new domain once and share it with damage collection and stability.
        assert_eq!(materializer.local_output_bounds_reads.get() - reads, 1);
    }
}

#[test]
fn fixed_isolated_inputs_skip_rechecks_but_domain_transitions_do_not() {
    use peniko::kurbo::Shape;
    let (mut scene, mut materializer) = fixture(true, 1);
    let clip = RetainedLayerDescriptor::ClipPath {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        rule: crate::FillRule::NonZero,
        tolerance: 0.1,
    };
    for (layer, snapshots) in [
        (layer(false, 1.0), 0),
        (clip, 1),
        (layer(false, 0.0), 1),
        (layer(false, 0.5), 0),
    ] {
        let before = materializer.input_domain_snapshots.get();
        scene
            .transaction()
            .update_layer(SOURCE, layer)
            .commit()
            .unwrap();
        update(&scene, &mut materializer, false);
        assert_eq!(
            materializer.input_domain_snapshots.get() - before,
            snapshots
        );
    }
}
