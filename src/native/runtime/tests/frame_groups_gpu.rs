use super::reference::{FilterVariant, FineVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::scene::SceneCache,
    renderer::{Execution, Images},
};
use crate::{Canvas, Mask, MaskKind, Radius, Region};
use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Rect, Shape},
};

fn group_canvas(kind: u32, path_region: bool) -> Canvas {
    let full = Rect::new(0.0, 0.0, 4.0, 2.0);
    let mut canvas = Canvas::new(4, 2, 1.0);
    if kind == 2 {
        canvas.push_rect(full, Radius::ZERO, Color::from_rgb8(128, 128, 128));
    }
    if kind == 0 {
        canvas.push_isolate_layer(full.to_path(0.25), Affine::IDENTITY, 0.25);
    }
    if kind == 1 {
        canvas.push_opacity_layer(full.to_path(0.25), Affine::IDENTITY, 0.25, 0.5);
    }
    if kind == 2 {
        canvas.push_blend_layer(
            full.to_path(0.25),
            Affine::IDENTITY,
            0.25,
            Mix::Multiply,
            Compose::SrcOver,
        );
    }
    let mut mask = Canvas::new(4, 2, 1.0);
    mask.push_rect(
        Rect::new(0.0, 0.0, 2.0, 2.0),
        Radius::ZERO,
        if kind == 3 {
            Color::from_rgb8(255, 0, 0)
        } else {
            Color::from_rgb8(255, 255, 255)
        },
    );
    let region = if path_region {
        let mut triangle = peniko::kurbo::BezPath::new();
        triangle.move_to((0.0, 0.0));
        triangle.line_to((4.0, 0.0));
        triangle.line_to((4.0, 2.0));
        triangle.close_path();
        Region::path(triangle, Affine::IDENTITY, 0.25)
    } else {
        Region::rect(full, Radius::ZERO)
    };
    canvas.push_mask_layer(
        mask,
        Mask {
            region,
            kind: if kind == 3 {
                MaskKind::Luminance
            } else {
                MaskKind::Alpha
            },
        },
    );
    canvas.push_rect(full, Radius::ZERO, Color::from_rgb8(255, 0, 0));
    canvas.pop_layer();
    if kind < 3 {
        canvas.pop_layer();
    }
    // A sibling forces released source/mask allocations to be cleared and reused.
    let mut mask = Canvas::new(4, 2, 1.0);
    mask.push_rect(
        Rect::new(3.0, 0.0, 4.0, 2.0),
        Radius::ZERO,
        Color::from_rgb8(255, 255, 255),
    );
    canvas.push_mask_layer(
        mask,
        Mask {
            region: Region::rect(full, Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    canvas.push_rect(full, Radius::ZERO, Color::from_rgb8(0, 0, 255));
    canvas.pop_layer();
    canvas
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_frame_groups_masks_and_scratch_reuse_preserve_pixels() -> Result<()> {
    let routes = super::fine_fixture::routes()?;
    let mut cache = SceneCache::default();
    for kind in 0..4 {
        let left = match kind {
            1 => [128, 0, 0, 128],
            3 => [54, 0, 0, 54],
            2 => [128, 0, 0, 255],
            _ => [255, 0, 0, 255],
        };
        let right = if kind == 2 {
            [128, 128, 128, 255]
        } else {
            [0; 4]
        };
        let rect_expected = vec![[left, left, right, [0, 0, 255, 255]].concat().repeat(2)];
        for path in [false, true] {
            for chunked in [false, true] {
                let canvas = group_canvas(kind, path);
                let expected = routes.canvas_reference(&canvas)?;
                if !path {
                    assert_eq!(expected, rect_expected);
                } else {
                    assert_ne!(expected, rect_expected, "path must remove covered pixels");
                }
                let mut batch = ComputeBatch::new();
                let upload = Default::default();
                let images = Images::record(&mut batch, &upload)?;
                let target = Execution::record(
                    &mut cache,
                    &mut batch,
                    &canvas,
                    &images,
                    None,
                    crate::native::runtime::renderer::FrameOptions {
                        chunked,
                        clear_color: 0,
                    },
                    65535,
                )?;
                assert!(
                    batch.outputs().is_empty(),
                    "all intermediate surfaces stay on GPU"
                );
                batch.readback(target)?;
                for fine in FineVariant::ALL {
                    routes.check_render(
                        &batch,
                        &expected,
                        &format!("frame group {kind} path {path} chunked {chunked}"),
                        FilterVariant {
                            portable: fine.portable,
                            texture_table: fine.texture_table,
                        },
                        fine,
                    )?;
                }
            }
        }
    }
    let canvas = Canvas::new(4, 2, 1.0);
    let expected = routes.canvas_reference(&canvas)?;
    assert_eq!(
        expected,
        vec![vec![0; 32]],
        "empty frame clears prior pixels"
    );
    let mut batch = ComputeBatch::new();
    let upload = Default::default();
    let images = Images::record(&mut batch, &upload)?;
    let target = Execution::record(
        &mut cache,
        &mut batch,
        &canvas,
        &images,
        None,
        crate::native::runtime::renderer::FrameOptions {
            chunked: false,
            clear_color: 0,
        },
        65535,
    )?;
    batch.readback(target)?;
    for fine in FineVariant::ALL {
        routes.check_render(
            &batch,
            &expected,
            "empty frame after groups",
            FilterVariant {
                portable: fine.portable,
                texture_table: fine.texture_table,
            },
            fine,
        )?;
    }
    routes.validate()
}
