//! Exercise version gaps through public submission errors and journal expiry.
use super::*;
use crate::{
    Filter, Radius, Region, RetainedLayerDescriptor, RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

const SOURCE: RetainedNodeId = RetainedNodeId::for_owner(2);

fn source(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(128, 64, 1.0);
    canvas.push_rect(Rect::new(16.0, 16.0, 32.0, 32.0), Radius::ZERO, color);
    Rc::new(canvas)
}

fn scene(domain: u32) -> Result<RetainedScene, Box<dyn std::error::Error>> {
    let root = RetainedNodeId::for_owner(1);
    let outer = RetainedNodeId::for_owner(3);
    let mut scene = RetainedScene::new(128, 64, 1.0, root)?;
    let mut tx = scene.transaction();
    let parent = if domain == 2 {
        tx.insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.8),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
            },
        );
        outer
    } else {
        root
    };
    tx.insert_scene(
        RetainedParent::content(parent),
        None,
        SOURCE,
        source(Color::WHITE),
        Affine::IDENTITY,
    );
    if domain != 0 {
        tx.insert_layer(
            RetainedParent::content(parent),
            None,
            RetainedNodeId::for_owner(4),
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Blur {
                    std_dev_x: 1.5,
                    std_dev_y: 1.5,
                    sampling: Default::default(),
                },
                sample_region: Region::rect(Rect::new(8.0, 8.0, 48.0, 48.0), Radius::ZERO),
            },
        );
        let mut foreground = Canvas::new(128, 64, 1.0);
        foreground.push_rect(
            Rect::new(24.0, 12.0, 28.0, 40.0),
            Radius::ZERO,
            Color::from_rgb8(20, 200, 60),
        );
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(5),
            Rc::new(foreground),
            Affine::IDENTITY,
        );
    }
    tx.commit()?;
    Ok(scene)
}

fn assert_fresh(
    renderer: &mut NativeRenderer,
    context: &NativeContext,
    scene: &RetainedScene,
    stage: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = renderer.render_retained_to_image(scene)?.readback()?;
    let mut fresh = NativeRenderer::with_context(context, 128, 64)?;
    let expected = fresh.render_retained_to_image(scene)?.readback()?;
    assert!(
        actual.pixels == expected.pixels,
        "{stage}: retained pixels differ from fresh rendering"
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_targets_preserve_damage_across_pruned_versions()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let rejected = context.create_texture(1, 1)?;
    for domain in 0..3 {
        let mut scene = scene(domain)?;
        let mut renderer = NativeRenderer::with_context(&context, 128, 64)?;
        assert_fresh(&mut renderer, &context, &scene, "initial")?;
        for color in [Color::from_rgb8(220, 20, 60), Color::from_rgb8(30, 60, 230)] {
            scene
                .transaction()
                .replace_scene(SOURCE, source(color))
                .commit()?;
            let error = renderer
                .render_retained_to_target(&scene, (&rejected).into())
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("native output size differs from canvas")
            );
        }
        // Both requests materialize new versions before target validation rejects them.
        // The node array can still be shared after the first state delta is pruned.
        assert_fresh(
            &mut renderer,
            &context,
            &scene,
            &format!("domain {domain}, rejected targets"),
        )?;
        scene
            .transaction()
            .replace_scene(SOURCE, source(Color::from_rgb8(240, 180, 20)))
            .commit()?;
        assert_fresh(
            &mut renderer,
            &context,
            &scene,
            &format!("domain {domain}, next incremental frame"),
        )?;
        assert!(!renderer.incremental_render_stats().full_redraw);
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn journal_expiry_rebuilds_backdrop_history_for_later_partial_frames()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    for domain in 1..3 {
        let mut scene = scene(domain)?;
        let mut renderer = NativeRenderer::with_context(&context, 128, 64)?;
        assert_fresh(&mut renderer, &context, &scene, "initial")?;
        let rendered = scene.version();
        for index in 0..=crate::retained_scene::JOURNAL_CAPACITY {
            scene
                .transaction()
                .replace_scene(SOURCE, source(Color::from_rgb8(index as u8, 40, 180)))
                .commit()?;
        }
        assert!(scene.changes_since(rendered).is_none());
        assert_fresh(&mut renderer, &context, &scene, "expired journal")?;
        assert!(renderer.incremental_render_stats().full_scene_sync);
        assert!(renderer.incremental_render_stats().full_redraw);
        scene
            .transaction()
            .replace_scene(SOURCE, source(Color::from_rgb8(50, 210, 70)))
            .commit()?;
        assert_fresh(
            &mut renderer,
            &context,
            &scene,
            "partial after journal expiry",
        )?;
        assert!(!renderer.incremental_render_stats().full_redraw);
        assert_fresh(&mut renderer, &context, &scene, "unchanged after recovery")?;
        assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
    }
    Ok(())
}
