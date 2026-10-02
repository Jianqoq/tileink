use super::*;
use crate::render::incremental::{IncrementalRenderConfig, IncrementalState};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

fn region() -> Region {
    Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO)
}
fn backdrop(amount: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Backdrop {
        filter: filter::Filter::Invert(amount),
        sample_region: region(),
    }
}
fn scene_with_parent(
    layer: RetainedLayerDescriptor,
) -> (RetainedScene, RetainedNodeId, RetainedNodeId) {
    let root = RetainedNodeId::for_owner(940_000);
    let parent = RetainedNodeId::for_owner(940_001);
    let child = RetainedNodeId::for_owner(940_002);
    let mut background = Canvas::new(64, 64, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(940_003),
            Rc::new(background),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(root), None, parent, layer)
        .insert_layer(RetainedParent::content(parent), None, child, backdrop(1.0))
        .commit()
        .unwrap();
    (scene, parent, child)
}

#[test]
fn parent_backdrop_parameter_changes_reach_its_later_child_backdrop() {
    let (mut scene, parent, child) = scene_with_parent(backdrop(0.0));
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    scene
        .transaction()
        .update_layer(parent, backdrop(1.0))
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let plan = state.plan(
        materializer.canvas.persistent_frame.clone(),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(!plan.stats.full_redraw);
    assert!(plan.dirty_backdrops.as_ref().unwrap().contains(&parent));
    assert!(
        plan.dirty_backdrops.as_ref().unwrap().contains(&child),
        "the parent's new backdrop output is already painted when its child samples input"
    );
}

#[test]
fn changing_from_fused_clip_to_isolation_invalidates_the_child_input_domain() {
    let path = Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1);
    let (mut scene, parent, child) = scene_with_parent(RetainedLayerDescriptor::ClipPath {
        path: path.clone(),
        transform: Affine::IDENTITY,
        rule: crate::FillRule::NonZero,
        tolerance: 0.1,
    });
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    scene
        .transaction()
        .update_layer(
            parent,
            RetainedLayerDescriptor::Isolate {
                path: path.clone(),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
            },
        )
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let plan = state.plan(
        materializer.canvas.persistent_frame.clone(),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(
        plan.stats.full_redraw || plan.dirty_backdrops.as_ref().unwrap().contains(&child),
        "matching child cache bounds do not preserve contents when its input changes to isolated scratch"
    );
    state.commit(materializer.canvas.persistent_frame.clone());
    scene
        .transaction()
        .update_layer(
            parent,
            RetainedLayerDescriptor::Opacity {
                path,
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let plan = state.plan(
        materializer.canvas.persistent_frame.clone(),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(
        !plan.stats.full_redraw,
        "remaining in the isolated input domain keeps the normal partial path"
    );
    assert!(!plan.dirty_backdrops.as_ref().unwrap().contains(&child));
}

#[test]
fn mixed_layer_and_leaf_updates_still_detect_changed_input_domains() {
    let path = Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1);
    let (mut scene, parent, child) = scene_with_parent(RetainedLayerDescriptor::ClipPath {
        path: path.clone(),
        transform: Affine::IDENTITY,
        rule: crate::FillRule::NonZero,
        tolerance: 0.1,
    });
    let marker = RetainedNodeId::for_owner(940_004);
    let leaf = |color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(
            Rect::new(48.0, 48.0, 49.0, 49.0),
            crate::Radius::ZERO,
            color,
        );
        Rc::new(canvas)
    };
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(RetainedNodeId::for_owner(940_000)),
            None,
            marker,
            leaf(Color::from_rgb8(255, 0, 0)),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    scene
        .transaction()
        .update_layer(
            parent,
            RetainedLayerDescriptor::Isolate {
                path,
                transform: Affine::IDENTITY,
                tolerance: 0.1,
            },
        )
        .replace_scene(marker, leaf(Color::from_rgb8(0, 0, 255)))
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let plan = state.plan(
        materializer.canvas.persistent_frame.clone(),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(
        plan.stats.full_redraw || plan.dirty_backdrops.as_ref().unwrap().contains(&child),
        "the transaction's input-domain change must not depend on a layer-only frame patch"
    );
}
