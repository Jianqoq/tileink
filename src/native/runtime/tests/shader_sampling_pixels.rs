use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_sampling_coordinates_survive_dpi_append_and_offscreen() -> Result<()> {
    use crate::{Canvas, Filter, NativeRenderer, Radius, Region};
    use peniko::{
        Color,
        kurbo::{Point, Rect},
    };
    let context = context()?;
    for scale in [1.0, 2.0] {
        for nested in [false, true] {
            let mut child = Canvas::new(48, 48, scale);
            let rect = Rect::new(4.0, 6.0, 36.0, 38.0);
            if nested {
                child.push_filter_layer(Filter::Opacity(1.0), Region::rect(rect, Radius::ZERO));
            }
            child.push_filter_layer(
                sampling([1.5, -0.5, 0.0, 0.0], 2.0)?.into(),
                Region::rect(rect, Radius::ZERO),
            );
            for y in 6..38 {
                for x in 4..36 {
                    child.push_rect(
                        Rect::new(x as f64, y as f64, x as f64 + 1.0, y as f64 + 1.0),
                        Radius::ZERO,
                        Color::from_rgb8(x as u8 * 5, y as u8 * 5, 80),
                    );
                }
            }
            child.pop_layer();
            if nested {
                child.pop_layer();
            }
            let mut parent = Canvas::new(56, 56, scale);
            parent.append(&child, Point::new(3.0, 2.0));
            let (w, h) = parent.physical_size();
            let mut renderer = NativeRenderer::with_context(&context, w, h)?;
            let image = renderer.render_to_image(&parent)?.readback()?;
            for y in (12.0 * scale) as usize..(30.0 * scale) as usize {
                for x in (12.0 * scale) as usize..(30.0 * scale) as usize {
                    let sx = x as f64 + 1.5 * f64::from(scale);
                    let sy = y as f64 - 0.5 * f64::from(scale);
                    let color = |x: f64, y: f64| {
                        [
                            ((x / f64::from(scale)).floor() - 3.0) * 5.0 / 255.0,
                            ((y / f64::from(scale)).floor() - 2.0) * 5.0 / 255.0,
                            80.0 / 255.0,
                            1.0,
                        ]
                    };
                    let (tx, ty) = (sx.fract(), sy.fract());
                    let mut expected = [0.0; 4];
                    for (dx, wx) in [(0.0, 1.0 - tx), (1.0, tx)] {
                        for (dy, wy) in [(0.0, 1.0 - ty), (1.0, ty)] {
                            for (v, c) in expected
                                .iter_mut()
                                .zip(color(sx.floor() + dx, sy.floor() + dy))
                            {
                                *v += c * wx * wy;
                            }
                        }
                    }
                    close(image.pixels[y * w as usize + x], pack(expected));
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_sampling_retained_neighbour_damage_matches_fresh_layers_and_backdrops()
-> Result<()> {
    use crate::{
        Canvas, NativeRenderer, Radius, Region, RetainedLayerDescriptor, RetainedNodeId,
        RetainedParent, RetainedScene,
    };
    use peniko::{
        Color,
        kurbo::{Affine, Rect},
    };
    use std::rc::Rc;
    let context = context()?;
    for backdrop in [false, true] {
        let root = RetainedNodeId::for_owner(1100);
        let layer = RetainedNodeId::for_owner(1101);
        let node = RetainedNodeId::for_owner(1102);
        let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
        let descriptor = |offset| -> Result<_> {
            let filter = sampling([offset, 0.0, 0.0, 0.0], 24.0)?.into();
            let rect = if backdrop {
                Rect::new(8.0, 8.0, 24.0, 32.0)
            } else {
                Rect::new(0.0, 0.0, 64.0, 64.0)
            };
            let sample_region = Region::rect(rect, Radius::ZERO);
            Ok(if backdrop {
                RetainedLayerDescriptor::Backdrop {
                    filter,
                    sample_region,
                }
            } else {
                RetainedLayerDescriptor::Filter {
                    filter,
                    sample_region,
                }
            })
        };
        let content = |color| {
            let mut c = Canvas::new(64, 64, 1.0);
            c.push_rect(Rect::new(30.0, 16.0, 34.0, 20.0), Radius::ZERO, color);
            Rc::new(c)
        };
        let parent = if backdrop { root } else { layer };
        let mut tx = scene.transaction();
        if !backdrop {
            tx.insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                descriptor(20.0)?,
            );
        }
        tx.insert_scene(
            RetainedParent::content(parent),
            None,
            node,
            content(Color::from_rgb8(180, 80, 120)),
            Affine::IDENTITY,
        );
        if backdrop {
            tx.insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                descriptor(20.0)?,
            );
            let mut foreground = Canvas::new(64, 64, 1.0);
            foreground.push_rect(
                Rect::new(32.0, 16.0, 34.0, 20.0),
                Radius::ZERO,
                Color::from_rgb8(10, 20, 240),
            );
            tx.insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(1103),
                Rc::new(foreground),
                Affine::IDENTITY,
            );
        }
        tx.commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
        for phase in 0..4 {
            if phase == 1 {
                scene
                    .transaction()
                    .replace_scene(node, content(Color::from_rgb8(80, 180, 120)))
                    .commit()?;
            }
            if phase == 2 {
                scene
                    .transaction()
                    .update_layer(layer, descriptor(18.0)?)
                    .commit()?;
            }
            let actual = renderer
                .render_retained_to_image(&scene)?
                .readback()?
                .pixels;
            let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?.pixels;
            let differences: Vec<_> = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(16)
                .map(|(i, (&a, &b))| (i % 64, i / 64, a, b))
                .collect();
            assert!(
                differences.is_empty(),
                "backdrop={backdrop} phase={phase} differences={differences:?}"
            );
            let x = if phase >= 2 { 13 } else { 11 };
            assert_eq!(
                actual[17 * 64 + x],
                if phase == 0 { 0xff7850b4 } else { 0xff78b450 }
            );
            assert_eq!(actual[17 * 64 + 3], 0);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_backdrop_samples_outside_its_output_region() -> Result<()> {
    use crate::{Canvas, NativeRenderer, Radius, Region};
    use peniko::{Color, kurbo::Rect};
    let context = context()?;
    let mut scene = Canvas::new(32, 32, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        Color::from_rgb8(20, 40, 60),
    );
    scene.push_rect(
        Rect::new(4.0, 10.0, 8.0, 14.0),
        Radius::ZERO,
        Color::from_rgb8(180, 80, 120),
    );
    scene.push_backdrop_layer(
        sampling([-4.0, 0.0, 0.0, 0.0], 4.0)?.into(),
        Region::rect(Rect::new(8.0, 8.0, 24.0, 24.0), Radius::ZERO),
    );
    scene.pop_layer();
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(image.pixels[11 * 32 + 9], 0xff7850b4);
    context.check_validation()?;
    Ok(())
}
