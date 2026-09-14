use super::{Result, fine_fixture::routes, reference::FineVariant, scene_coarse::encode_canvas};
use crate::{
    Canvas, Image, PatternSampling,
    native::runtime::{
        compute::{ComputeBatch, Resource},
        program::scene::SceneCache,
    },
    shared::{
        gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
        image_resource::{ImageKey, ImageResourceStore},
    },
};
use peniko::{
    Extend,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

fn vector_batch(wide: bool, sampling: PatternSampling) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let (width, height, atlas_limit, capacity) = if wide {
        (2050, 1, 4096, NATIVE_TEXTURE_TABLE_CAPACITY)
    } else {
        (4, 4, 8, 0)
    };
    let mut child = Canvas::new(width, height, 1.0);
    let midpoint = f64::from(width / 2);
    for (x0, x1, color) in [
        (0.0, midpoint, [255, 0, 0]),
        (midpoint, f64::from(width), [0, 255, 0]),
    ] {
        child.push_path(
            Rect::new(x0, 0.0, x1, f64::from(height)).to_path(0.25),
            crate::Brush::Solid(peniko::Color::from_rgb8(color[0], color[1], color[2])),
            Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.25,
        );
    }
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Rc::new(child));
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        atlas_limit,
        4,
        capacity,
        None,
    );
    let mut raster_pixels = Vec::new();
    for _ in 0..height {
        raster_pixels.extend([255, 0, 0, 255].repeat((width / 2) as usize));
        raster_pixels.extend([0, 255, 0, 255].repeat((width / 2) as usize));
    }
    let mut raster = ImageResourceStore::default();
    raster.insert(ImageKey(1), Image::from_rgba8(width, height, raster_pixels));
    let raster_upload = raster.upload_merged(
        &ImageResourceStore::default(),
        atlas_limit,
        4,
        capacity,
        None,
    );
    let expected_image = if wide {
        assert_eq!(upload.textures().len(), 1);
        bytemuck::cast_slice::<_, u8>(&raster_upload.textures()[0].pixels).to_vec()
    } else {
        assert_eq!(upload.atlas_page_count(), 1);
        bytemuck::cast_slice::<_, u8>(&raster_upload.atlas_pages()[0].pixels).to_vec()
    };
    let mut batch = ComputeBatch::new();
    let images = crate::native::runtime::renderer::Images::record_with_vectors(
        &mut batch,
        &upload,
        |batch, child| {
            let empty = Default::default();
            let images = crate::native::runtime::renderer::Images::record(batch, &empty)?;
            encode_canvas(&mut SceneCache::default(), batch, child, &images, false)
        },
    )?;
    let mut parent = Canvas::new(2, 2, 1.0);
    parent
        .push_image_key(
            Rect::new(0.0, 0.0, 2.0, 2.0),
            ImageKey(1),
            Extend::Pad,
            sampling,
        )
        .unwrap();
    let target = encode_canvas(
        &mut SceneCache::default(),
        &mut batch,
        &parent,
        &images,
        false,
    )?;
    assert!(batch.outputs().is_empty());
    batch.readback(target)?;
    let image = if wide {
        let Resource::TextureTable(table) = &batch.resources()[images.textures().table.index()]
        else {
            unreachable!()
        };
        table[upload.textures()[0].index as usize]
    } else {
        images.textures().atlas
    };
    batch.readback(image)?;
    Ok((
        batch,
        vec![[255, 0, 0, 255, 0, 255, 0, 255].repeat(2), expected_image],
    ))
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_canvas_vector_images_preserve_pixels_and_borders() -> Result<()> {
    let routes = routes()?;
    for wide in [false, true] {
        for sampling in [PatternSampling::Nearest, PatternSampling::Bilinear] {
            let (batch, expected) = vector_batch(wide, sampling)?;
            for variant in FineVariant::ALL {
                if wide && !variant.texture_table {
                    continue;
                }
                routes.check_fine(
                    &batch,
                    &expected,
                    "Canvas vector image with raster-equivalent pixels/borders",
                    variant,
                )?;
            }
        }
    }
    routes.validate()
}
