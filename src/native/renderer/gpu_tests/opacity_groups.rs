use super::{Canvas, NativeRenderer, context};
use crate::{Radius, RetainedLayerDescriptor, RetainedNodeId, RetainedParent, RetainedScene};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn leaf(scale: f32) -> Rc<Canvas> {
    // A fragment's layout/viewport is not an implicit clip of its recorded paint.
    let mut canvas = Canvas::new(8, 4, scale);
    canvas.push_rect(
        Rect::new(8.0, 8.0, 32.0, 40.0),
        Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(20.0, 12.0, 48.0, 36.0),
        Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_clip_sdf_rect_layer(Rect::new(24.0, 16.0, 44.0, 32.0), Radius::ZERO);
    canvas.push_rect(
        Rect::new(16.0, 16.0, 52.0, 32.0),
        Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.pop_layer();
    Rc::new(canvas)
}

const fn alpha_at(pixels: &[u32], scale: u32, x: u32, y: u32) -> u8 {
    pixels[((y * scale) * 64 * scale + x * scale) as usize].to_le_bytes()[3]
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn unbounded_opacity_applies_once_preserves_overflow_and_matches_retained_updates() -> TestResult {
    let context = context()?;
    for scale in [1, 2] {
        let root = RetainedNodeId::for_owner(1);
        let group = RetainedNodeId::for_owner(2);
        let child = RetainedNodeId::for_owner(3);
        let mut scene = RetainedScene::new(64, 64, scale as f32, root)?;
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                group,
                RetainedLayerDescriptor::OpacityGroup { opacity: 0.5 },
            )
            .insert_scene(
                RetainedParent::content(group),
                None,
                child,
                leaf(scale as f32),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
        for opacity in [0.5, 0.25, 0.75, 0.0, 1.0, 0.5] {
            scene
                .transaction()
                .update_layer(group, RetainedLayerDescriptor::OpacityGroup { opacity })
                .commit()?;
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
            assert!(
                actual.pixels == fresh.render_retained_to_image(&scene)?.readback()?.pixels,
                "retained differs from fresh"
            );
            for (x, y) in [(12, 12), (22, 14), (28, 24), (46, 24)] {
                let alpha = alpha_at(&actual.pixels, scale, x, y);
                assert!(
                    alpha.abs_diff((opacity * 255.0).round() as u8) <= 1,
                    "scale={scale}, alpha={opacity}, point=({x},{y}): {alpha}"
                );
            }
            // Independently recorded immediate groups must use the same semantics.
            let mut immediate = Canvas::new(64, 64, scale as f32);
            immediate.push_opacity_group(opacity);
            immediate.append(&leaf(scale as f32), peniko::kurbo::Point::ZERO);
            immediate.pop_layer();
            let mut direct = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
            assert!(
                actual.pixels == direct.render_to_image(&immediate)?.readback()?.pixels,
                "retained differs from immediate: alpha={opacity}, scale={scale}"
            );
        }
        // Replacing a small, distant leaf must not apply alpha again to clean tiles
        // in the already-composited source. Bounds stay stable across these edits.
        let corner_id = RetainedNodeId::for_owner(6);
        let corner = |color| {
            let mut canvas = Canvas::new(64, 64, scale as f32);
            canvas.push_rect(Rect::new(52.0, 52.0, 60.0, 60.0), Radius::ZERO, color);
            Rc::new(canvas)
        };
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(group),
                None,
                corner_id,
                corner(Color::from_rgb8(255, 0, 0)),
                Affine::IDENTITY,
            )
            .commit()?;
        renderer.render_retained_to_image(&scene)?.readback()?;
        for color in [Color::from_rgb8(0, 0, 255), Color::from_rgb8(255, 0, 0)] {
            scene
                .transaction()
                .replace_scene(corner_id, corner(color))
                .commit()?;
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
            assert!(
                actual.pixels == fresh.render_retained_to_image(&scene)?.readback()?.pixels,
                "partial cached group update at scale={scale}"
            );
            assert!(renderer.incremental_render_stats().dirty_tiles < 16 * scale * scale);
        }
        // Moving cached children changes the work bounds and clears their old output.
        for offset in [(8.0, 4.0), (-4.0, 8.0), (0.25, 0.5)] {
            scene
                .transaction()
                .set_transform(child, Affine::translate(offset))
                .commit()?;
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
            assert!(
                actual.pixels == fresh.render_retained_to_image(&scene)?.readback()?.pixels,
                "retained differs from fresh"
            );
        }
        // An authored clip is still effective when an opacity group is unbounded.
        let clip = RetainedNodeId::for_owner(4);
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                clip,
                RetainedLayerDescriptor::ClipPath {
                    path: Rect::new(0.0, 0.0, 30.0, 64.0).to_path(0.1),
                    transform: Affine::IDENTITY,
                    rule: crate::FillRule::NonZero,
                    tolerance: 0.1,
                },
            )
            .reparent(group, RetainedParent::content(clip), None)
            .commit()?;
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
        assert!(
            actual.pixels == fresh.render_retained_to_image(&scene)?.readback()?.pixels,
            "retained differs from fresh"
        );
        assert_eq!(alpha_at(&actual.pixels, scale, 40, 24), 0);
        assert!(alpha_at(&actual.pixels, scale, 28, 24) > 0);
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_unbounded_opacity_inside_a_local_filter_matches_direct_pixels() -> TestResult {
    use crate::{Filter, Region};
    let context = context()?;
    let root = RetainedNodeId::for_owner(1);
    let group = RetainedNodeId::for_owner(2);
    let child = RetainedNodeId::for_owner(3);
    let nested = RetainedNodeId::for_owner(4);
    let filter = RetainedNodeId::for_owner(5);
    for scale in [1, 2] {
        let region = Region::rect(Rect::new(8.0, 8.0, 56.0, 56.0), Radius::ZERO);
        let mut scene = RetainedScene::new(64, 64, scale as f32, root)?;
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                filter,
                RetainedLayerDescriptor::Filter {
                    filter: Filter::Opacity(1.0),
                    sample_region: region.clone(),
                },
            )
            .insert_layer(
                RetainedParent::content(filter),
                None,
                group,
                RetainedLayerDescriptor::OpacityGroup { opacity: 0.5 },
            )
            .insert_layer(
                RetainedParent::content(group),
                None,
                nested,
                RetainedLayerDescriptor::OpacityGroup { opacity: 0.5 },
            )
            .insert_scene(
                RetainedParent::content(nested),
                None,
                child,
                leaf(scale as f32),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
        for opacity in [0.5, 0.25, 1.0, 0.0, 0.5] {
            scene
                .transaction()
                .update_layer(group, RetainedLayerDescriptor::OpacityGroup { opacity })
                .commit()?;
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut direct = Canvas::new(64, 64, scale as f32);
            direct.push_filter_layer(Filter::Opacity(1.0), region.clone());
            direct.push_opacity_group(opacity);
            direct.push_opacity_group(0.5);
            direct.append(&leaf(scale as f32), peniko::kurbo::Point::ZERO);
            for _ in 0..3 {
                direct.pop_layer();
            }
            let mut fresh = NativeRenderer::with_context(&context, 64 * scale, 64 * scale)?;
            assert!(
                actual.pixels == fresh.render_to_image(&direct)?.readback()?.pixels,
                "nested local group: opacity={opacity}, scale={scale}"
            );
            for (x, y) in [(12, 12), (22, 14), (28, 24), (46, 24)] {
                assert!(
                    alpha_at(&actual.pixels, scale, x, y)
                        .abs_diff((opacity * 0.5 * 255.0).round() as u8)
                        <= 1
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}
