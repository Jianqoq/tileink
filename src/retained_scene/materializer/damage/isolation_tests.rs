use super::*;
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

fn assert_outside_input_is_isolated(layer: RetainedLayerDescriptor) {
    let root = RetainedNodeId::for_owner(937_000);
    let outside = RetainedNodeId::for_owner(937_001);
    let group = RetainedNodeId::for_owner(937_002);
    let backdrop = RetainedNodeId::for_owner(937_004);
    let rect = Rect::new(16.0, 16.0, 32.0, 32.0);
    let leaf = |color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            outside,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(root), None, group, layer)
        .insert_scene(
            RetainedParent::content(group),
            None,
            RetainedNodeId::for_owner(937_003),
            leaf(Color::BLACK),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(group),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: filter::Filter::Invert(1.0),
                sample_region: Region::rect(rect, crate::Radius::ZERO),
            },
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let old = materializer.version().get();
    scene
        .transaction()
        .replace_scene(outside, leaf(Color::from_rgb8(0, 255, 0)))
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let current = materializer.canvas.persistent_frame.as_ref().unwrap();
    let damage = current
        .damage_history
        .resolve(old, current.version.unwrap())
        .unwrap()
        .unwrap();
    assert!(
        !damage.dirty_backdrops.contains(&backdrop),
        "a composited group has its own backdrop input texture"
    );
}
#[test]
fn opacity_blocks_outside_damage_from_its_internal_backdrop() {
    assert_outside_input_is_isolated(RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity: 0.5,
    });
}
#[test]
fn blend_blocks_outside_damage_from_its_internal_backdrop() {
    assert_outside_input_is_isolated(RetainedLayerDescriptor::Blend {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        mix: peniko::Mix::Multiply,
        compose: peniko::Compose::SrcOver,
    });
}
#[test]
fn isolate_blocks_outside_damage_from_its_internal_backdrop() {
    assert_outside_input_is_isolated(RetainedLayerDescriptor::Isolate {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
    });
}
#[test]
fn mask_content_does_not_invalidate_its_independent_mask_branch_backdrop() {
    let root = RetainedNodeId::for_owner(938_000);
    let mask = RetainedNodeId::for_owner(938_001);
    let content = RetainedNodeId::for_owner(938_002);
    let backdrop = RetainedNodeId::for_owner(938_003);
    let rect = Rect::new(16.0, 16.0, 32.0, 32.0);
    let leaf = |color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            mask,
            RetainedLayerDescriptor::Mask(crate::Mask {
                region: Region::rect(rect, crate::Radius::ZERO),
                kind: crate::MaskKind::Alpha,
            }),
        )
        .insert_scene(
            RetainedParent::content(mask),
            None,
            content,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::mask(mask),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: filter::Filter::Invert(1.0),
                sample_region: Region::rect(rect, crate::Radius::ZERO),
            },
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let old = materializer.version().get();
    scene
        .transaction()
        .replace_scene(content, leaf(Color::BLACK))
        .commit()
        .unwrap();
    let changes = scene.changes_since(materializer.version());
    materializer.update(&scene, changes);
    let current = materializer.canvas.persistent_frame.as_ref().unwrap();
    let damage = current
        .damage_history
        .resolve(old, current.version.unwrap())
        .unwrap()
        .unwrap();
    assert!(
        !damage.dirty_backdrops.contains(&backdrop),
        "content is rendered into a different texture than the mask branch"
    );
}
