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
