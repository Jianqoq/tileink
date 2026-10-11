#[path = "support/native_gpu.rs"]
mod backend;

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
