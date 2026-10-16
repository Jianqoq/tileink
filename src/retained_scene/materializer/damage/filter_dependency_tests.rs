use super::*;
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

fn nested_off_canvas_scene() -> (RetainedScene, RetainedNodeId) {
    let root = RetainedNodeId::for_owner(929_300);
    let outer = RetainedNodeId::for_owner(929_301);
    let inner = RetainedNodeId::for_owner(929_302);
    let leaf = RetainedNodeId::for_owner(929_303);
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(
        Rect::new(-16.0, 0.0, -15.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: filter::Filter::ConvolveMatrix(filter::ConvolveMatrix {
                    columns: 3,
                    rows: 1,
                    target_x: 1,
                    target_y: 0,
                    data: vec![1.0, 0.0, 0.0],
                    divisor: 1.0,
                    bias: 0.0,
                    edge_mode: filter::ConvolveEdgeMode::Wrap,
                    preserve_alpha: false,
                }),
                sample_region: Region::Rect {
                    rect: Rect::new(-16.0, 0.0, 48.0, 64.0),
                    radius: crate::Radius::ZERO,
                },
            },
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            inner,
            RetainedLayerDescriptor::Filter {
                filter: filter::Filter::Opacity(1.0),
                sample_region: Region::Rect {
                    rect: Rect::new(-16.0, 0.0, -15.0, 64.0),
                    radius: crate::Radius::ZERO,
                },
            },
        )
        .insert_scene(
            RetainedParent::content(inner),
            None,
            leaf,
            Rc::new(child),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    (scene, leaf)
}

#[test]
fn nested_off_canvas_damage_reaches_the_visible_ancestor_output() {
    let (scene, leaf) = nested_off_canvas_scene();
    let materializer = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        materializer.influenced_bounds(&scene, leaf, Bounds::new(-16, 0, -15, 64)),
        Bounds::new(0, 0, 48, 64)
    );
}

#[test]
fn composed_influence_preserves_off_canvas_intermediate_filter_output() {
    let (scene, leaf) = nested_off_canvas_scene();
    let materializer = PersistentSceneMaterializer::new(&scene);
    let mut influences = HashMap::default();
    materializer.collect_bounds_influences(
        &scene,
        scene.root,
        BoundsInfluence {
            outset: 0,
            clip: Some(Bounds::canvas(64, 64)),
        },
        &mut influences,
    );
    assert_eq!(
        influences[&leaf].apply(Bounds::new(-16, 0, -15, 64)),
        Bounds::new(0, 0, 48, 64)
    );
}

#[test]
fn composing_a_farther_filter_cannot_revive_an_empty_nearer_clip() {
    let farther = BoundsInfluence {
        outset: 64,
        clip: Some(Bounds::canvas(64, 64)),
    };
    let nearer = BoundsInfluence {
        outset: 0,
        clip: Some(Bounds::new(16, 0, 16, 64)),
    };
    assert!(
        farther
            .with_nearer(nearer)
            .apply(Bounds::new(15, 0, 17, 64))
            .is_empty()
    );
}

#[test]
fn retained_filter_damage_uses_the_encoded_physical_filter_at_high_dpi() {
    let root = RetainedNodeId::for_owner(929_320);
    let layer = RetainedNodeId::for_owner(929_321);
    let leaf = RetainedNodeId::for_owner(929_322);
    let mut child = Canvas::new(96, 64, 2.0);
    child.push_rect(
        Rect::new(8.0, 8.0, 10.0, 10.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    let mut scene = RetainedScene::new(96, 64, 2.0, root).unwrap();
    let descriptor = RetainedLayerDescriptor::Filter {
        filter: filter::Filter::Offset { dx: 4.0, dy: -2.0 },
        sample_region: Region::rect(Rect::new(8.0, 8.0, 18.0, 14.0), crate::Radius::ZERO),
    };
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            descriptor.clone(),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            Rc::new(child),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let materializer = PersistentSceneMaterializer::new(&scene);
    let changed = Bounds::new(16, 16, 20, 20);
    let expected = Bounds::new(8, 8, 28, 28);
    assert_eq!(
        materializer.influenced_bounds(&scene, leaf, changed),
        expected
    );
    assert_eq!(
        materializer
            .local_bounds_influence(layer, &descriptor)
            .apply(changed),
        expected
    );
}

#[test]
fn retained_mask_damage_uses_its_encoded_physical_region_at_high_dpi() {
    let root = RetainedNodeId::for_owner(929_330);
    let layer = RetainedNodeId::for_owner(929_331);
    let leaf = RetainedNodeId::for_owner(929_332);
    let mut child = Canvas::new(96, 64, 2.0);
    child.push_rect(
        Rect::new(8.0, 8.0, 10.0, 10.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    let mut scene = RetainedScene::new(96, 64, 2.0, root).unwrap();
    let descriptor = RetainedLayerDescriptor::Mask(crate::Mask {
        kind: crate::MaskKind::Alpha,
        region: Region::rect(Rect::new(8.0, 8.0, 18.0, 14.0), crate::Radius::ZERO),
    });
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            descriptor.clone(),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            Rc::new(child),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let materializer = PersistentSceneMaterializer::new(&scene);
    let changed = Bounds::new(16, 16, 20, 20);
    assert_eq!(
        materializer.influenced_bounds(&scene, leaf, changed),
        changed
    );
    assert_eq!(
        materializer
            .local_bounds_influence(layer, &descriptor)
            .apply(changed),
        changed
    );
}
