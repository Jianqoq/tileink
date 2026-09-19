use super::super::Result;
use crate::{
    Canvas, IncrementalRenderConfig, IncrementalRenderMode, NativeBackend, NativeContext,
    NativeContextOptions, NativeRenderer, Radius, RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::Color;
use peniko::kurbo::{Affine, Rect};
use std::rc::Rc;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_retained_updates_static_frames_and_removal_match_force_full() -> Result<()> {
    let routes = super::fine_fixture::routes()?;
    let child = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
        Rc::new(canvas)
    };
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let mut auto = NativeRenderer::with_context(&context, 64, 32)?;
        let mut full = NativeRenderer::with_context(&context, 64, 32)?;
        full.set_incremental_render_config(IncrementalRenderConfig {
            mode: IncrementalRenderMode::ForceFull,
            ..Default::default()
        });
        let root = RetainedNodeId::for_owner(900_000);
        let node = RetainedNodeId::for_owner(900_001);
        let mut scene = RetainedScene::new(64, 32, 1.0, root)?;
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                node,
                child(Color::from_rgba8(211, 37, 71, 137)),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut reference = crate::retained_scene::PersistentSceneMaterializer::new(&scene);
        for frame in 0..4 {
            if frame == 2 {
                scene
                    .transaction()
                    .replace_scene(node, child(Color::from_rgba8(31, 193, 79, 211)))
                    .commit()?;
            } else if frame == 3 {
                scene.transaction().remove_subtree(node).commit()?;
            }
            reference.update(&scene, scene.changes_since(reference.version()));
            let expected = routes.canvas_reference(&reference.canvas())?;
            let image = auto.render_retained_to_image(&scene)?.readback()?;
            let force_full = full.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(&image.pixels),
                expected[0],
                "{backend:?} frame {frame}"
            );
            assert_eq!(image.pixels, force_full.pixels, "Auto versus ForceFull");
            let stats = auto.incremental_render_stats();
            if frame > 0 {
                assert!(
                    !stats.full_redraw,
                    "expected retained partial frame {frame}: {stats:?}"
                );
            }
            if frame == 1 {
                assert_eq!(stats.dirty_tiles, 0);
            }
        }
        // A different renderer may write the same target without editing this scene.
        // Content versions must invalidate cached damage before the next retained frame.
        let target = context.create_texture(64, 32)?;
        auto.render_retained_to_texture(&scene, &target)?.wait()?;
        let mut foreign = NativeRenderer::with_context(&context, 64, 32)?;
        foreign.set_clear_color(Color::from_rgba8(11, 29, 43, 255));
        foreign
            .render_to_texture(&Canvas::new(64, 32, 1.0), &target)?
            .wait()?;
        auto.render_retained_to_texture(&scene, &target)?.wait()?;
        assert!(auto.incremental_render_stats().full_redraw);
        assert!(
            target
                .readback()?
                .readback()?
                .pixels
                .iter()
                .all(|pixel| *pixel == 0)
        );
        context.check_validation()?;
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_retained_output_rectangles_preserve_sentinels_and_history_contracts() -> Result<()> {
    use crate::{ExternalTextureHistoryId, NativeRenderTarget};
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let texture = context.create_texture(24, 20)?;
        let mut writer = NativeRenderer::with_context(&context, 24, 20)?;
        writer.set_clear_color(Color::from_rgba8(11, 29, 43, 255));
        writer
            .render_to_texture(&Canvas::new(24, 20, 1.0), &texture)?
            .wait()?;
        let root = RetainedNodeId::for_owner(910_000);
        let node = RetainedNodeId::for_owner(910_001);
        let mut scene = RetainedScene::new(16, 12, 1.0, root)?;
        let mut child = Canvas::new(16, 12, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, 16.0, 12.0),
            Radius::ZERO,
            Color::from_rgba8(211, 37, 71, 255),
        );
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                node,
                Rc::new(child),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 16, 12)?;
        for transient in [false, true] {
            let target = if transient {
                NativeRenderTarget::transient(&texture)
            } else {
                NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(1))
            }
            .with_origin(3, 5);
            for frame in 0..2 {
                renderer.render_retained_to_target(&scene, target)?.wait()?;
                if frame == 1 {
                    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
                }
                let image = texture.readback()?.readback()?;
                for y in 0..20 {
                    for x in 0..24 {
                        let expected = if (3..19).contains(&x) && (5..17).contains(&y) {
                            [211, 37, 71, 255]
                        } else {
                            [11, 29, 43, 255]
                        };
                        assert_eq!(image.rgba8_at(x, y), expected, "{backend:?} at {x},{y}");
                    }
                }
                if transient {
                    writer
                        .render_to_texture(&Canvas::new(24, 20, 1.0), &texture)?
                        .wait()?;
                }
            }
        }
        let fresh = NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(2))
            .with_origin(3, 5);
        renderer.render_retained_to_target(&scene, fresh)?.wait()?;
        assert!(renderer.incremental_render_stats().full_redraw);
        assert!(
            renderer
                .render_retained_to_target(&scene, fresh.with_origin(u32::MAX, 5))
                .is_err()
        );
        renderer.render_retained_to_target(&scene, fresh)?.wait()?;
        assert_eq!(
            renderer.incremental_render_stats().dirty_tiles,
            0,
            "rejected rectangle must not alter valid history"
        );
        context.check_validation()?;
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_retained_text_mode_and_raster_changes_invalidate_static_history() -> Result<()> {
    use crate::{TextContext, TextLayoutOptions, TextRasterOptions, TextSubpixelMode};
    use peniko::kurbo::Point;
    let routes = super::fine_fixture::routes()?;
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let mut fonts = super::frame_text::frame_fonts();
        let mut text = TextContext::new();
        let layout = text.layout(&mut fonts, TextLayoutOptions::new("Ag7", 19.0));
        let mut canvas = Canvas::new(83, 61, 1.0);
        canvas.push_text_layout(
            &layout,
            Point::new(1.25, 3.5),
            Color::from_rgb8(31, 67, 111),
        );
        let root = RetainedNodeId::for_owner(920_000);
        let mut scene = RetainedScene::new(83, 61, 1.0, root)?;
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(920_001),
                Rc::new(canvas),
                Affine::IDENTITY,
            )
            .commit()?;
        let reference = crate::retained_scene::PersistentSceneMaterializer::new(&scene);
        let mut auto = NativeRenderer::with_context(&context, 83, 61)?;
        let mut full = NativeRenderer::with_context(&context, 83, 61)?;
        full.set_incremental_render_config(IncrementalRenderConfig {
            mode: IncrementalRenderMode::ForceFull,
            ..Default::default()
        });
        for (frame, enabled) in [false, true, true, true, false].into_iter().enumerate() {
            if frame == 3 {
                text.clear_glyph_caches();
            } else if enabled {
                text.set_raster_options(TextRasterOptions::new().with_subpixel_mode(
                    if text.raster_options().subpixel_mode == TextSubpixelMode::Bgr {
                        TextSubpixelMode::Rgb
                    } else {
                        TextSubpixelMode::Bgr
                    },
                ));
            }
            let (actual, expected, wgpu) = if enabled {
                (
                    auto.render_retained_to_image_with_text(&scene, &mut fonts, &mut text)?
                        .readback()?,
                    full.render_retained_to_image_with_text(&scene, &mut fonts, &mut text)?
                        .readback()?,
                    routes.text_reference(&reference.canvas(), &mut fonts, &mut text)?,
                )
            } else {
                (
                    auto.render_retained_to_image(&scene)?.readback()?,
                    full.render_retained_to_image(&scene)?.readback()?,
                    routes.canvas_reference(&reference.canvas())?,
                )
            };
            assert!(
                auto.incremental_render_stats().full_redraw,
                "text environment changed at frame {frame}"
            );
            assert_eq!(
                actual.pixels, expected.pixels,
                "text input changed without a scene edit"
            );
            assert_eq!(bytemuck::cast_slice::<_, u8>(&actual.pixels), wgpu[0]);
        }
        context.check_validation()?;
    }
    routes.validate()
}
