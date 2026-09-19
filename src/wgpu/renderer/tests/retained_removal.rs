use super::*;
use peniko::kurbo::Shape;
use std::rc::Rc;

#[test]
fn retained_removal_and_reinsertion_refresh_backdrop_history() {
    if !run_wgpu_tests() {
        return;
    }
    let region = Region::rect(Rect::new(0.0, 0.0, 128.0, 96.0), crate::Radius::ZERO);
    for (domain, outer) in [
        ("root", None),
        (
            "filter",
            Some(RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.75),
                sample_region: region.clone(),
            }),
        ),
        (
            "isolate",
            Some(RetainedLayerDescriptor::Isolate {
                path: Rect::new(0.0, 0.0, 128.0, 96.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
            }),
        ),
        (
            "mask",
            Some(RetainedLayerDescriptor::Mask(crate::Mask {
                region,
                kind: crate::MaskKind::Alpha,
            })),
        ),
    ] {
        check_removal_history(domain, outer, false);
    }
}

fn check_removal_history(
    domain: &str,
    outer: Option<RetainedLayerDescriptor>,
    late_foreground: bool,
) {
    let (width, height) = (128, 96);
    let root = RetainedNodeId::for_owner(892_100);
    let background = RetainedNodeId::for_owner(892_101);
    let clip = RetainedNodeId::for_owner(892_102);
    let changing = RetainedNodeId::for_owner(892_103);
    let backdrop = RetainedNodeId::for_owner(892_104);
    let solid = |rect, color| {
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        Rc::new(canvas)
    };
    let content = solid(
        Rect::new(32.25, 32.75, 52.5, 48.5),
        Color::from_rgba8(230, 70, 30, 201),
    );
    let parent = if outer.is_some() {
        RetainedNodeId::for_owner(892_105)
    } else {
        root
    };
    let masked = matches!(&outer, Some(RetainedLayerDescriptor::Mask(_)));
    let mut scene = RetainedScene::new(width, height, 1.0, root).unwrap();
    if let Some(layer) = outer {
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, parent, layer)
            .commit()
            .unwrap();
    }
    if masked {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::mask(parent),
                None,
                RetainedNodeId::for_owner(892_106),
                solid(
                    Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                    Color::WHITE,
                ),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
    }
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(parent),
            None,
            background,
            solid(
                Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                Color::from_rgb8(30, 55, 91),
            ),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(parent),
            None,
            clip,
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(30.0, 30.0, 56.0, 52.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: crate::FillRule::NonZero,
                tolerance: 0.1,
            },
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            changing,
            content.clone(),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(parent),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Blur {
                    std_dev_x: 2.25,
                    std_dev_y: 1.75,
                    sampling: Default::default(),
                },
                sample_region: Region::rect(Rect::new(0.0, 0.0, 96.0, 80.0), crate::Radius::ZERO),
            },
        )
        .commit()
        .unwrap();
    if late_foreground {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(parent),
                None,
                RetainedNodeId::for_owner(892_107),
                solid(
                    Rect::new(24.0, 64.0, 76.0, 80.0),
                    Color::from_rgba8(229, 233, 239, 213),
                ),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
    }
    let mut incremental = new_test_renderer(width, height, Color::TRANSPARENT);
    let mut full = new_test_renderer(width, height, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    for step in 0..3 {
        match step {
            1 => {
                scene
                    .transaction()
                    .remove_subtree(changing)
                    .commit()
                    .unwrap();
            }
            2 => {
                scene
                    .transaction()
                    .insert_scene(
                        RetainedParent::content(parent),
                        Some(clip),
                        changing,
                        content.clone(),
                        Affine::translate((2.5, -1.25)),
                    )
                    .commit()
                    .unwrap();
            }
            _ => {}
        }
        if late_foreground && step > 0 {
            // Scratch history is unspecified. Dirty output must depend only on
            // this frame's initialized intermediate samples, including its halo.
            for scratch in incremental
                .scratch
                .iter()
                .enumerate()
                .filter(|(index, _)| !incremental.scratch_slots.is_occupied(*index))
                .map(|(_, target)| target)
                .chain(incremental.scratch_spares.iter())
            {
                let texture = scratch.texture();
                let poison = vec![0xff00ffff_u32; (texture.width() * texture.height()) as usize];
                incremental.queue.write_texture(
                    texture.as_image_copy(),
                    bytemuck::cast_slice(&poison),
                    ::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(texture.width() * 4),
                        rows_per_image: Some(texture.height()),
                    },
                    texture.size(),
                );
            }
        }
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        if step > 0 {
            assert!(
                !incremental.incremental_render_stats().full_redraw,
                "{domain} step {step}: {:?}",
                incremental.incremental_render_stats()
            );
        }
        let actual = incremental.image();
        let expected = full.image();
        if let Some((index, (actual, expected))) = actual
            .pixels
            .iter()
            .zip(&expected.pixels)
            .enumerate()
            .find(|(_, (a, b))| a != b)
        {
            panic!(
                "{domain} step {step}: pixel ({}, {}) actual {actual:#010x}, expected {expected:#010x}",
                index % width as usize,
                index / width as usize
            );
        }
    }
}

#[test]
fn retained_partial_blur_writes_vertical_halo_before_sampling() {
    if !run_wgpu_tests() {
        return;
    }
    // The unchanged next tile contains a later foreground. It must never become
    // input to this backdrop when the preceding tile is partially redrawn.
    check_removal_history("post-backdrop foreground", None, true);
}
