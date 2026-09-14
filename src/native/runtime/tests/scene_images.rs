use super::*;
use crate::native::runtime::compute::Resource;
use crate::shared::image_resource::{ImageKey, ImageResourceStore};
use crate::{Canvas, Image};
use std::rc::Rc;

#[test]
fn empty_images_have_initialized_atlas_and_complete_table() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let images = SceneImages::record(&mut batch, &GpuImageResourceUpload::default())?;
    let Resource::Texture(atlas) = &batch.resources()[images.atlas.index()] else {
        panic!()
    };
    assert_eq!(atlas.bytes, [0; 4]);
    assert!(atlas.array);
    let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
        panic!()
    };
    assert_eq!(table.len(), NATIVE_TEXTURE_TABLE_CAPACITY as usize);
    assert!(table.iter().all(|id| *id == table[0]));
    Ok(())
}

#[test]
fn image_upload_preserves_atlas_borders_and_table_pixels() -> Result<()> {
    let mut store = ImageResourceStore::default();
    store.insert(
        ImageKey(1),
        Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 255, 0, 128]),
    );
    store.insert(
        ImageKey(2),
        Image::from_rgba8(2050, 1, [0, 0, 255, 255].repeat(2050)),
    );
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        4096,
        4,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    assert!(!upload.atlas_pages().is_empty());
    assert_eq!(upload.textures().len(), 1);
    let mut batch = ComputeBatch::new();
    let images = SceneImages::record(&mut batch, &upload)?;
    let Resource::Texture(atlas) = &batch.resources()[images.atlas.index()] else {
        panic!()
    };
    let expected: Vec<u8> = upload
        .atlas_pages()
        .iter()
        .flat_map(|p| bytemuck::cast_slice::<_, u8>(&p.pixels).iter().copied())
        .collect();
    assert_eq!(atlas.bytes, expected);
    let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
        panic!()
    };
    for texture in upload.textures() {
        let Resource::Texture(actual) = &batch.resources()[table[texture.index as usize].index()]
        else {
            panic!()
        };
        assert_eq!(actual.bytes, bytemuck::cast_slice::<_, u8>(&texture.pixels));
    }
    Ok(())
}

#[test]
fn unresolved_vectors_are_rejected_before_resources_are_recorded() {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Rc::new(Canvas::new(2, 2, 1.0)));
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        4096,
        4,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    assert_eq!(upload.vectors().len(), 1);
    let mut batch = ComputeBatch::new();
    assert!(SceneImages::record(&mut batch, &upload).is_err());
    assert!(batch.resources().is_empty());
}

#[test]
fn malformed_atlas_pages_are_rejected_before_recording() {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Image::from_rgba8(1, 1, [255; 4]));
    let original = store.upload_merged(
        &ImageResourceStore::default(),
        4096,
        4,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    for field in 0..3 {
        let mut upload = original.clone();
        let page = &mut upload.atlas_pages_mut()[0];
        match field {
            0 => page.index += 1,
            1 => page.size += 1,
            _ => {
                page.pixels.pop();
            }
        }
        let mut batch = ComputeBatch::new();
        assert!(SceneImages::record(&mut batch, &upload).is_err());
        assert!(batch.resources().is_empty());
    }
}

#[test]
fn fresh_vector_targets_are_filled_even_with_clean_cached_placements() -> Result<()> {
    let mut store = ImageResourceStore::default();
    let canvas = Rc::new(Canvas::new(4, 4, 1.0));
    store.insert(ImageKey(1), canvas.clone());
    let original = store.upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);
    let cached = store.upload_merged(&ImageResourceStore::default(), 8, 4, 0, Some(&original));
    assert!(!cached.vectors()[0].dirty);
    let mut batch = ComputeBatch::new();
    let mut rendered = 0;
    SceneImages::record_with_vectors(&mut batch, &cached, |batch, child| {
        assert!(Rc::ptr_eq(child, &canvas));
        rendered += 1;
        batch.texture_rgba8([4, 4], vec![0; 4 * 4 * 4])
    })?;
    assert_eq!(rendered, 1);
    assert_eq!(batch.commands().len(), 9);
    assert!(batch.outputs().is_empty());
    Ok(())
}

#[test]
fn vector_output_failures_never_return_ready_image_handles() {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Rc::new(Canvas::new(4, 4, 1.0)));
    let upload = store.upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);
    let mut batch = ComputeBatch::new();
    assert!(
        SceneImages::record_with_vectors(&mut batch, &upload, |_, _| Err(
            "child render failed".into()
        ))
        .is_err()
    );
    assert!(batch.commands().is_empty());
    for invalid in 0..3 {
        let mut batch = ComputeBatch::new();
        let result =
            SceneImages::record_with_vectors(&mut batch, &upload, |batch, _| match invalid {
                0 => batch.texture_rgba8([1, 1], vec![0; 4]),
                1 => ComputeBatch::new().texture_rgba8([4, 4], vec![0; 64]),
                _ => batch.texture_array_rgba8([4, 4, 1], vec![0; 64]),
            });
        assert!(result.is_err());
        assert!(batch.commands().is_empty());
    }
}

#[test]
fn vector_texture_without_cpu_pixels_gets_complete_gpu_storage() -> Result<()> {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Rc::new(Canvas::new(2050, 1, 1.0)));
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        4096,
        4,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    assert_eq!(upload.textures().len(), 1);
    assert!(upload.textures()[0].pixels.is_empty());
    let mut batch = ComputeBatch::new();
    let images = SceneImages::record_with_vectors(&mut batch, &upload, |batch, _| {
        batch.texture_rgba8([2050, 1], vec![0; 2050 * 4])
    })?;
    let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
        panic!()
    };
    let Resource::Texture(texture) =
        &batch.resources()[table[upload.textures()[0].index as usize].index()]
    else {
        panic!()
    };
    assert_eq!(texture.bytes.len(), 2050 * 4);
    assert_eq!(batch.commands().len(), 1);
    Ok(())
}

#[test]
fn oversized_vector_dimensions_are_rejected_before_pixel_allocation() {
    // Empty Canvas construction and vector placement create metadata only. This
    // must fail dimension validation before attempting a 16 GiB pixel allocation.
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey(1), Rc::new(Canvas::new(u32::MAX, 1, 1.0)));
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        u32::MAX,
        1,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    assert_eq!(upload.textures()[0].width, u32::MAX);
    let result = SceneImages::record_with_vectors(&mut ComputeBatch::new(), &upload, |_, _| {
        panic!("invalid destination cannot invoke child rendering")
    });
    assert!(result.is_err());
}
