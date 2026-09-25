use super::*;
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

#[test]
fn changing_a_nested_filter_invalidates_its_off_canvas_backdrop_before_outer_offset() {
    let root = RetainedNodeId::for_owner(930_000);
    let outer = RetainedNodeId::for_owner(930_001);
    let source_filter = RetainedNodeId::for_owner(930_002);
    let backdrop = RetainedNodeId::for_owner(930_004);
    let rect = Rect::new(-16.0, 16.0, 0.0, 32.0);
    let region = || Region::Rect {
        rect,
        radius: crate::Radius::ZERO,
    };
    let descriptor = |amount| RetainedLayerDescriptor::Filter {
        filter: filter::Filter::Invert(amount),
        sample_region: region(),
    };
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(rect, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: filter::Filter::Offset { dx: 16.0, dy: 0.0 },
                sample_region: region(),
            },
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            source_filter,
            descriptor(0.0),
        )
        .insert_scene(
            RetainedParent::content(source_filter),
            None,
            RetainedNodeId::for_owner(930_003),
            Rc::new(child),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: filter::Filter::Invert(1.0),
                sample_region: region(),
            },
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    for amount in [1.0, 0.5] {
        scene
            .transaction()
            .update_layer(source_filter, descriptor(amount))
            .commit()
            .unwrap();
        let changes = scene.changes_since(materializer.version());
        assert!(materializer.update(&scene, changes));
        let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
        let delta = frame
            .delta
            .as_ref()
            .expect("stable filter parameters take the actual frame patch path");
        assert!(!delta.backdrop_damage_complete);
        let resolved = frame
            .damage_history
            .resolve(delta.from_version, delta.to_version)
            .unwrap()
            .unwrap();
        assert!(
            resolved.dirty_backdrops.contains(&backdrop),
            "the backdrop samples changed local input before the outer filter moves it into the root"
        );
        assert!(
            resolved
                .bounds
                .iter()
                .any(|bounds| !bounds.intersect(Bounds::new(0, 16, 16, 32)).is_empty()),
            "changed backdrop output must then propagate through the outer filter"
        );
    }
}

#[test]
fn consecutive_opacity_updates_keep_layer_output_bounds() {
    use peniko::kurbo::Shape;
    let root = RetainedNodeId::for_owner(931_000);
    let outer = RetainedNodeId::for_owner(931_001);
    let opacity = RetainedNodeId::for_owner(931_002);
    let rect = Rect::new(-16.0, 16.0, 0.0, 32.0);
    let descriptor = |amount| RetainedLayerDescriptor::Opacity {
        path: rect.to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity: amount,
    };
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(rect, crate::Radius::ZERO, Color::WHITE);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: filter::Filter::Offset { dx: 16.0, dy: 0.0 },
                sample_region: Region::rect(rect, crate::Radius::ZERO),
            },
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            opacity,
            descriptor(1.0),
        )
        .insert_scene(
            RetainedParent::content(opacity),
            None,
            RetainedNodeId::for_owner(931_003),
            Rc::new(child),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    for amount in [0.5, 0.25] {
        scene
            .transaction()
            .update_layer(opacity, descriptor(amount))
            .commit()
            .unwrap();
        let changes = scene.changes_since(materializer.version());
        assert!(materializer.update(&scene, changes));
        let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
        assert!(
            frame.delta.is_some(),
            "stable opacity parameters use the frame patch path"
        );
        let output = frame.node_state(opacity).unwrap().bounds;
        assert!(
            !output.intersect(Bounds::new(0, 16, 16, 32)).is_empty(),
            "a layer chunk is a shell, not its empty child scene; amount={amount}, output={output:?}"
        );
    }
}

#[test]
fn expanding_an_ancestor_clip_invalidates_the_backdrop_input_it_reveals() {
    use crate::render::incremental::{IncrementalRenderConfig, IncrementalState};
    use peniko::kurbo::Shape;
    let root = RetainedNodeId::for_owner(935_000);
    let clip = RetainedNodeId::for_owner(935_001);
    let backdrop = RetainedNodeId::for_owner(935_003);
    let full = Rect::new(0.0, 0.0, 32.0, 16.0);
    let descriptor = |rect: Rect| RetainedLayerDescriptor::ClipPath {
        path: rect.to_path(0.1),
        transform: Affine::IDENTITY,
        rule: crate::FillRule::NonZero,
        tolerance: 0.1,
    };
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            clip,
            descriptor(Rect::new(0.0, 0.0, 16.0, 16.0)),
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            RetainedNodeId::for_owner(935_002),
            Rc::new(child),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(clip),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: filter::Filter::Invert(1.0),
                sample_region: Region::rect(full, crate::Radius::ZERO),
            },
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    scene
        .transaction()
        .update_layer(clip, descriptor(full))
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    assert!(materializer.update(&scene, changes));
    let plan = state.plan(
        materializer.canvas.persistent_frame.clone(),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(!plan.stats.full_redraw);
    assert!(
        plan.dirty_backdrops.as_ref().unwrap().contains(&backdrop),
        "changing ancestor coverage changes the earlier painted input seen by its descendant backdrop"
    );
}
