#[path = "support/native_gpu.rs"]
mod backend;

#[test]
#[ignore = "requires pinned TILEINK_TEST_GPU; run each backend separately"]
fn large_clip_bounds_survive_filter_surface_offsets() {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };
    use tileink::{Canvas, FillRule, Filter, Radius, Region};
    let mut gpu = backend::Gpu::new();
    let target = gpu.target(32, 32);
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    let clip = Rect::new(-1.0e10, -1.0e10, 1.0e10, 1.0e10);
    for (dx, dy) in [(4, 4), (-4, -4), (4, -4), (-4, 4)] {
        for path in [false, true] {
            let mut canvas = Canvas::new(32, 32, 1.0);
            canvas.push_filter_layer(
                Filter::Offset {
                    dx: dx as f32,
                    dy: dy as f32,
                },
                Region::rect(full, Radius::ZERO),
            );
            if path {
                // Fractional edges keep the rectangle on the path clip route.
                canvas.push_clip_layer(
                    clip.to_path(0.1),
                    Affine::translate((0.25, 0.25)),
                    FillRule::NonZero,
                    0.1,
                );
            } else {
                canvas.push_clip_sdf_rect_layer(clip, Radius::ZERO);
            }
            canvas.push_rect(full, Radius::ZERO, Color::WHITE);
            canvas.pop_layer();
            canvas.pop_layer();
            gpu.render_immediate(&canvas, &target);
            let image = gpu.image_target(&target);
            for y in 0..32 {
                for x in 0..32 {
                    let inside =
                        (0..32).contains(&(x as i32 - dx)) && (0..32).contains(&(y as i32 - dy));
                    assert_eq!(
                        image.pixels[y * 32 + x],
                        if inside { 0xffff_ffff } else { 0 },
                        "path={path}, offset=({dx}, {dy}), ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires pinned TILEINK_TEST_GPU; run each backend separately"]
fn deep_offscreen_clips_preserve_filter_mask_and_group_pixels() {
    use peniko::{
        Color, Compose, Mix,
        kurbo::{Affine, Rect, Shape},
    };
    use tileink::{Canvas, FillRule, Filter, Mask, MaskKind, Radius, Region};
    let mut gpu = backend::Gpu::new();
    let target = gpu.target(32, 32);
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    let clip = Rect::new(8.0, 6.0, 24.0, 26.0);
    for depth in [1, 63, 64, 65, 128] {
        for masked in [false, true] {
            let mut canvas = Canvas::new(32, 32, 1.0);
            for index in 0..depth {
                if index % 2 == 0 {
                    canvas.push_clip_sdf_rect_layer(clip, Radius::ZERO);
                } else {
                    canvas.push_clip_layer(
                        clip.to_path(0.1),
                        Affine::IDENTITY,
                        FillRule::NonZero,
                        0.1,
                    );
                }
            }
            canvas.push_opacity_layer(full.to_path(0.1), Affine::IDENTITY, 0.1, 0.5);
            canvas.push_blend_layer(
                full.to_path(0.1),
                Affine::IDENTITY,
                0.1,
                Mix::Normal,
                Compose::SrcOver,
            );
            if masked {
                let mut mask = Canvas::new(32, 32, 1.0);
                mask.push_rect(full, Radius::ZERO, Color::WHITE);
                canvas.push_mask_layer(
                    mask,
                    Mask {
                        region: Region::rect(full, Radius::ZERO),
                        kind: MaskKind::Alpha,
                    },
                );
            } else {
                canvas.push_filter_layer(
                    Filter::Offset { dx: 0.0, dy: 0.0 },
                    Region::rect(full, Radius::ZERO),
                );
            }
            canvas.push_rect(full, Radius::ZERO, Color::WHITE);
            for _ in 0..depth + 3 {
                canvas.pop_layer();
            }
            gpu.render_immediate(&canvas, &target);
            let image = gpu.image_target(&target);
            for y in 0..32 {
                for x in 0..32 {
                    let expected = if clip.contains((x as f64 + 0.5, y as f64 + 0.5)) {
                        0x80808080
                    } else {
                        0
                    };
                    assert_eq!(
                        image.pixels[y * 32 + x],
                        expected,
                        "depth={depth}, masked={masked}, ({x},{y})"
                    );
                }
            }
        }
    }
}
