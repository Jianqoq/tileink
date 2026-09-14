use super::common::*;
use peniko::kurbo::Shape;

fn region() -> Region {
    Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO)
}
fn backdrop(amount: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Backdrop {
        filter: Filter::Invert(amount),
        sample_region: region(),
    }
}
fn assert_parent_update(
    old: RetainedLayerDescriptor,
    new: RetainedLayerDescriptor,
    expect_partial: bool,
    mixed: bool,
) {
    let root = RetainedNodeId::for_owner(941_000);
    let parent = RetainedNodeId::for_owner(941_001);
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
            RetainedNodeId::for_owner(941_003),
            std::rc::Rc::new(background),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(root), None, parent, old)
        .insert_layer(
            RetainedParent::content(parent),
            None,
            RetainedNodeId::for_owner(941_002),
            backdrop(1.0),
        )
        .commit()
        .unwrap();
    let marker = RetainedNodeId::for_owner(941_004);
    let leaf = |color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(
            Rect::new(48.0, 48.0, 49.0, 49.0),
            crate::Radius::ZERO,
            color,
        );
        std::rc::Rc::new(canvas)
    };
    if mixed {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                marker,
                leaf(Color::from_rgb8(255, 0, 0)),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
    }
    let mut incremental = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut full = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(full.image().rgba8_at(8, 8), [0, 0, 0, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    let mut transaction = scene.transaction();
    transaction.update_layer(parent, new);
    if mixed {
        transaction.replace_scene(marker, leaf(Color::from_rgb8(0, 0, 255)));
    }
    transaction.commit().unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    if expect_partial {
        assert!(!incremental.incremental_render_stats().full_redraw);
    }
    assert_eq!(full.image().rgba8_at(8, 8), [255, 255, 255, 255]);
    assert_eq!(
        incremental.image().pixels,
        full.image().pixels,
        "the descendant backdrop must consume the new parent output/input domain"
    );
}
#[test]
fn parent_backdrop_parameter_update_matches_full_for_its_descendant() {
    if run_wgpu_tests() {
        assert_parent_update(backdrop(0.0), backdrop(1.0), true, false);
    }
}
#[test]
fn replacing_clip_with_isolate_restores_the_child_backdrop_input() {
    if !run_wgpu_tests() {
        return;
    }
    let path = Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1);
    assert_parent_update(
        RetainedLayerDescriptor::ClipPath {
            path: path.clone(),
            transform: Affine::IDENTITY,
            rule: crate::FillRule::NonZero,
            tolerance: 0.1,
        },
        RetainedLayerDescriptor::Isolate {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
        },
        false,
        false,
    );
}

#[test]
fn mixed_layer_and_scene_update_restores_the_child_backdrop_input() {
    if !run_wgpu_tests() {
        return;
    }
    let path = Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1);
    assert_parent_update(
        RetainedLayerDescriptor::ClipPath {
            path: path.clone(),
            transform: Affine::IDENTITY,
            rule: crate::FillRule::NonZero,
            tolerance: 0.1,
        },
        RetainedLayerDescriptor::Isolate {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
        },
        false,
        true,
    );
}
