use super::*;
use crate::{
    Canvas, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind, NativeRenderer, Radius,
    Region, RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Point, Rect},
};
use std::rc::Rc;

#[path = "shader_retained.rs"]
mod retained;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_brush_and_filter_coordinates_survive_dpi_append_and_offscreen() -> Result<()> {
    let context = context()?;
    let program = coordinates()?;
    let uniform = [[1.0 / 64.0, 1.0 / 64.0, 0.0, 1.0]];
    let bounds = Rect::new(7.0, 9.0, 39.0, 41.0);
    for scale in [1.0, 2.0] {
        let mut direct = Canvas::new(48, 48, scale);
        direct.push_rect(
            bounds,
            Radius::ZERO,
            program.brush(super::uniform(&uniform))?,
        );
        let mut filter = Canvas::new(48, 48, scale);
        filter.push_filter_layer(
            program.filter(super::uniform(&uniform))?.into(),
            Region::rect(bounds, Radius::ZERO),
        );
        filter.push_rect(bounds, Radius::ZERO, Color::WHITE);
        filter.pop_layer();
        let mut offscreen = Canvas::new(48, 48, scale);
        offscreen.push_filter_layer(Filter::Opacity(1.0), Region::rect(bounds, Radius::ZERO));
        offscreen.push_rect(
            bounds,
            Radius::ZERO,
            program.brush(super::uniform(&uniform))?,
        );
        offscreen.pop_layer();
        let mut renderer = NativeRenderer::with_context(&context, 96, 96)?;
        for scene in [&direct, &filter, &offscreen] {
            let mut parent = Canvas::new(48, 48, scale);
            parent.append(scene, Point::new(3.0, 2.0));
            let image = renderer.render_to_image(&parent)?.readback()?;
            let (w, h) = parent.physical_size();
            for y in 0..h {
                for x in 0..w {
                    let px = (f64::from(x) + 0.5) / f64::from(scale) - 3.0;
                    let py = (f64::from(y) + 0.5) / f64::from(scale) - 2.0;
                    let expected = if (7.0..39.0).contains(&px) && (9.0..41.0).contains(&py) {
                        pack([px / 64.0, py / 64.0, 0.0, 1.0])
                    } else {
                        0
                    };
                    close(image.pixels[(y * w + x) as usize], expected);
                }
            }
        }
    }
    let transform = Affine::translate((46.0, 0.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    let mut renderer = NativeRenderer::with_context(&context, 48, 48)?;
    {
        let mut child = Canvas::new(48, 48, 1.0);
        child.push_rect(
            bounds,
            Radius::ZERO,
            program.brush(super::uniform(&uniform))?,
        );
        let mut parent = Canvas::new(48, 48, 1.0);
        parent.append_transformed(&child, transform);
        let image = renderer.render_to_image(&parent)?.readback()?;
        for y in 8..38 {
            for x in 6..36 {
                let p = transform.inverse() * Point::new(x as f64 + 0.5, y as f64 + 0.5);
                close(
                    image.pixels[y * 48 + x],
                    pack([p.x / 64.0, p.y / 64.0, 0.0, 1.0]),
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_point_filters_handle_alpha_chains_graphs_and_backdrops() -> Result<()> {
    let context = context()?;
    let program = input_scale()?;
    let bounds = Rect::new(8.0, 8.0, 40.0, 40.0);
    let region = Region::rect(bounds, Radius::ZERO);
    let uniform = [[0.5, 0.75, 0.25, 0.5]];
    for alpha in [0, 1, 128, 254, 255] {
        let color = Color::from_rgba8(80, 120, 200, alpha);
        let mut source = Canvas::new(48, 48, 1.0);
        source.push_rect(bounds, Radius::ZERO, color);
        let mut renderer = NativeRenderer::with_context(&context, 48, 48)?;
        let raw = renderer.render_to_image(&source)?.readback()?.pixels[20 * 48 + 20].to_le_bytes();
        let expected = pack(std::array::from_fn(|i| {
            f64::from(raw[i]) / 255.0 * f64::from(uniform[0][i])
        }));
        for mode in 0..5 {
            let effect: Filter = program.filter(super::uniform(&uniform))?.into();
            let filter = match mode {
                1 => Filter::Chain {
                    filters: vec![
                        Filter::Flood {
                            brush: color.into(),
                        },
                        effect,
                    ],
                    fixed_region: true,
                },
                2 | 3 => Filter::Graph {
                    primitives: vec![FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: crate::Bounds::new(8, 8, 40, 40),
                        linear_rgb: mode == 3,
                        kind: FilterPrimitiveKind::Filter(Box::new(effect)),
                    }],
                    fixed_region: true,
                },
                _ => effect,
            };
            let mut scene = Canvas::new(48, 48, 1.0);
            if mode == 4 {
                scene.push_rect(bounds, Radius::ZERO, color);
                scene.push_backdrop_layer(filter, region.clone());
            } else {
                scene.push_filter_layer(filter, region.clone());
                scene.push_rect(bounds, Radius::ZERO, color);
            }
            scene.pop_layer();
            let actual = renderer.render_to_image(&scene)?.readback()?;
            let expected = if mode == 4 {
                let filtered = expected.to_le_bytes();
                let inv = 255 - u32::from(filtered[3]);
                u32::from_le_bytes(std::array::from_fn(|i| {
                    let product = u32::from(raw[i]) * inv + 128;
                    (u32::from(filtered[i]) + ((product + (product >> 8)) >> 8)) as u8
                }))
            } else {
                expected
            };
            close(actual.pixels[20 * 48 + 20], expected);
            assert_eq!(actual.pixels[0], 0);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_retained_uniform_and_program_replacements_match_fresh_frames() -> Result<()> {
    let context = context()?;
    let paint = coordinates()?;
    let point = input_scale()?;
    let root = RetainedNodeId::for_owner(800);
    let node = RetainedNodeId::for_owner(801);
    let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
    let mut previous = None;
    for phase in 0..5 {
        let mut child = Canvas::new(48, 48, 1.0);
        let rect = Rect::new(0.0, 0.0, 32.0, 32.0);
        let mut effect = point.filter(super::uniform(&[[1.0, 1.0, 1.0, 1.0]]))?;
        if phase > 0 {
            effect = effect.with_resource(
                3,
                crate::ShaderResource::Buffer(super::uniform(&[[0.5, 1.0, 1.0, 0.75]]).into()),
            )?;
        }
        child.push_filter_layer(effect.into(), Region::rect(rect, Radius::all(3.0)));
        if phase < 3 {
            child.push_rect(
                rect,
                Radius::ZERO,
                paint
                    .brush(super::uniform(&[[1.0 / 32.0, 1.0 / 32.0, 0.0, 1.0]]))?
                    .with_uniforms(super::uniform(&[[
                        1.0 / (32.0 + phase as f32 * 8.0),
                        1.0 / 32.0,
                        0.0,
                        1.0,
                    ]]))?,
            );
        } else {
            child.push_rect(rect, Radius::ZERO, Color::from_rgba8(200, 60, 120, 180));
        }
        child.pop_layer();
        let mut tx = scene.transaction();
        if phase == 0 {
            tx.insert_scene(
                RetainedParent::content(root),
                None,
                node,
                Rc::new(child),
                Affine::IDENTITY,
            );
        } else {
            tx.replace_scene(node, Rc::new(child));
        }
        tx.set_transform(
            node,
            Affine::translate((f64::from(phase) * 3.0, f64::from(phase) * 2.0)),
        )
        .commit()?;
        let actual = renderer
            .render_retained_to_image(&scene)?
            .readback()?
            .pixels;
        let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
        assert_eq!(
            actual,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels
        );
        if let Some(previous) = previous {
            assert_ne!(actual, previous);
        }
        previous = Some(actual);
    }
    scene.transaction().remove_subtree(node).commit()?;
    assert!(
        renderer
            .render_retained_to_image(&scene)?
            .readback()?
            .pixels
            .iter()
            .all(|&p| p == 0)
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_brush_flood_and_clip_preserve_coordinates_and_coverage() -> Result<()> {
    let context = context()?;
    let brush = coordinates()?.brush(super::uniform(&[[1.0 / 64.0, 1.0 / 64.0, 0.0, 1.0]]))?;
    let bounds = Rect::new(7.0, 9.0, 39.0, 41.0);
    let mut direct = Canvas::new(48, 48, 1.0);
    direct.push_rect(bounds, Radius::ZERO, brush.clone());
    let mut flood = Canvas::new(48, 48, 1.0);
    flood.push_filter_layer(
        Filter::Flood {
            brush: brush.clone().into(),
        },
        Region::rect(bounds, Radius::ZERO),
    );
    flood.push_rect(bounds, Radius::ZERO, Color::WHITE);
    flood.pop_layer();
    let mut renderer = NativeRenderer::with_context(&context, 48, 48)?;
    assert_eq!(
        renderer.render_to_image(&direct)?.readback()?.pixels,
        renderer.render_to_image(&flood)?.readback()?.pixels
    );
    let clip_rect = Rect::new(12.0, 14.0, 34.0, 36.0);
    let mut clipped = Canvas::new(48, 48, 1.0);
    clipped.push_clip_sdf_rect_layer(clip_rect, Radius::all(5.0));
    clipped.push_rect(bounds, Radius::ZERO, brush);
    clipped.pop_layer();
    let mut coverage = Canvas::new(48, 48, 1.0);
    coverage.push_clip_sdf_rect_layer(clip_rect, Radius::all(5.0));
    coverage.push_rect(bounds, Radius::ZERO, Color::WHITE);
    coverage.pop_layer();
    let color = renderer.render_to_image(&clipped)?.readback()?.pixels;
    let mask = renderer.render_to_image(&coverage)?.readback()?.pixels;
    for (i, (&pixel, &mask)) in color.iter().zip(&mask).enumerate() {
        assert_eq!(pixel >> 24, mask >> 24, "coverage pixel {i}");
        if mask == u32::MAX {
            close(
                pixel,
                pack([
                    ((i % 48) as f64 + 0.5) / 64.0,
                    ((i / 48) as f64 + 0.5) / 64.0,
                    0.0,
                    1.0,
                ]),
            );
        }
    }
    context.check_validation()?;
    Ok(())
}
