use super::reference::{FilterVariant, FineVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    renderer::recording::{Limits, Recording},
};
use crate::shared::image_resource::ImageResourceStore;
use crate::{Brush, Canvas, Filter, Image, PatternSampling, Radius, Region};
use peniko::Extend;
use peniko::kurbo::Rect;
use std::rc::Rc;

pub(super) fn nested_scene(color: [u8; 4]) -> Canvas {
    let bounds = Rect::new(0.0, 0.0, 19.0, 13.0);
    let mut leaf = Canvas::new(19, 13, 1.0);
    leaf.push_image(
        bounds,
        Image::from_rgba8(1, 1, color),
        Extend::Pad,
        PatternSampling::Nearest,
    )
    .unwrap();
    for depth in 0..2 {
        let mut parent = Canvas::new(19, 13, 1.0);
        let key = parent.register_scene_image(Rc::new(leaf)).unwrap();
        let brush = Brush::from_scene_image_key_with_options(
            key,
            bounds,
            Extend::Pad,
            PatternSampling::Bilinear,
            255,
        )
        .unwrap();
        if depth == 1 {
            parent.push_filter_layer(Filter::Opacity(0.7), Region::rect(bounds, Radius::ZERO));
        }
        parent.push_rect(bounds, Radius::ZERO, brush);
        if depth == 1 {
            parent.pop_layer();
        }
        leaf = parent;
    }
    leaf
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_recording_prepares_nested_images_and_reuses_metadata() -> Result<()> {
    let routes = super::fine_fixture::routes()?;
    let mut recording = Recording::default();
    let resources = ImageResourceStore::default();
    let limits = Limits {
        image_dimension: 64,
        atlas_pages: 4,
        texture_table_len: 0,
        dispatch_dimension: 65535,
    };
    for color in [[241, 53, 17, 255], [31, 173, 223, 255]] {
        let canvas = nested_scene(color);
        let expected = routes.canvas_reference(&canvas)?;
        for chunked in [false, true] {
            let mut batch = ComputeBatch::new();
            let target =
                recording.record(&mut batch, &canvas, &resources, None, limits, chunked)?;
            assert!(
                batch.outputs().is_empty(),
                "production recording must not read back"
            );
            batch.readback(target)?;
            for fine in FineVariant::ALL {
                routes.check_render(
                    &batch,
                    &expected,
                    "assembled nested image frame",
                    FilterVariant {
                        portable: fine.portable,
                        texture_table: fine.texture_table,
                    },
                    fine,
                )?;
            }
        }
    }
    // A numeric image key is meaningful only within its namespace. The public
    // assembly boundary must preserve both entries when merging resource stores.
    let mut canvas = Canvas::new(4, 2, 1.0);
    let left = Rect::new(0.0, 0.0, 2.0, 2.0);
    let right = Rect::new(2.0, 0.0, 4.0, 2.0);
    let scene_image = Rc::new(Image::from_rgba8(1, 1, [239, 17, 53, 255]));
    let renderer_image = Rc::new(Image::from_rgba8(1, 1, [13, 197, 71, 255]));
    let key = canvas.register_scene_image(scene_image.clone()).unwrap();
    let brush = Brush::from_scene_image_key_with_options(
        key,
        left,
        Extend::Pad,
        PatternSampling::Nearest,
        255,
    )
    .unwrap();
    canvas.push_rect(left, Radius::ZERO, brush);
    canvas
        .push_image_key(right, key, Extend::Pad, PatternSampling::Nearest)
        .unwrap();
    let mut resources = ImageResourceStore::default();
    resources.insert(key, renderer_image.clone());
    let mut reference = Canvas::new(4, 2, 1.0);
    reference
        .push_image(left, scene_image, Extend::Pad, PatternSampling::Nearest)
        .unwrap();
    reference
        .push_image(right, renderer_image, Extend::Pad, PatternSampling::Nearest)
        .unwrap();
    let expected = routes.canvas_reference(&reference)?;
    let mut batch = ComputeBatch::new();
    let target = recording.record(&mut batch, &canvas, &resources, None, limits, false)?;
    batch.readback(target)?;
    for fine in FineVariant::ALL {
        routes.check_render(
            &batch,
            &expected,
            "image namespace collision",
            FilterVariant {
                portable: fine.portable,
                texture_table: fine.texture_table,
            },
            fine,
        )?;
    }
    routes.validate()
}
