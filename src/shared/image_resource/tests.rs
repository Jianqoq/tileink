use super::{
    ImageKey, ImageResourceId, ImageResourcePlacement, ImageResourceStore,
    MAX_IMAGE_RESOURCE_TEXTURES,
};
use crate::shared::image::Image;

#[test]
fn remove_and_clear_update_image_resource_uploads() {
    let mut resources = ImageResourceStore::default();
    let first = ImageKey::new(1);
    let second = ImageKey::new(2);

    assert!(resources.insert(first, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
    assert!(resources.insert(second, Image::from_rgba8(1, 1, [0, 255, 0, 255])));
    let scene = ImageResourceStore::default();
    let upload = resources.upload_merged(&scene, 4096, 256, 0, None);
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(first))
            .is_some()
    );
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(second))
            .is_some()
    );

    assert!(resources.remove(first));
    assert!(!resources.remove(first));
    let upload = resources.upload_merged(&scene, 4096, 256, 0, Some(&upload));
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(first))
            .is_none()
    );
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(second))
            .is_some()
    );

    assert!(resources.clear());
    assert!(!resources.clear());
    let upload = resources.upload_merged(&scene, 4096, 256, 0, Some(&upload));
    assert_eq!(upload.atlas_page_size(), 1);
    assert_eq!(upload.atlas_page_count(), 1);
    assert_eq!(upload.atlas_pages()[0].pixels.len(), 1);
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(second))
            .is_none()
    );
}

#[test]
fn small_images_pack_into_multiple_atlas_pages() {
    let mut resources = ImageResourceStore::default();
    for key in 1..=4 {
        assert!(resources.insert(
            ImageKey::new(key),
            solid_image(4, 4, [key as u8, 0, 0, 255])
        ));
    }
    let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, None);

    assert_eq!(upload.atlas_page_size(), 8);
    assert_eq!(upload.atlas_page_count(), 4);
    for key in 1..=4 {
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(ImageKey::new(key))),
            Some(ImageResourcePlacement::Atlas(_))
        ));
    }
}

#[test]
fn atlas_page_count_is_capped_by_device_layer_limit() {
    let mut resources = ImageResourceStore::default();
    for key in 1..=4 {
        assert!(resources.insert(
            ImageKey::new(key),
            solid_image(4, 4, [key as u8, 0, 0, 255])
        ));
    }

    let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 2, 0, None);

    assert_eq!(upload.atlas_page_count(), 2);
    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(ImageKey::new(1))),
        Some(ImageResourcePlacement::Atlas(rect)) if rect.page < 2
    ));
    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(ImageKey::new(2))),
        Some(ImageResourcePlacement::Atlas(rect)) if rect.page < 2
    ));
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(ImageKey::new(3)))
            .is_none()
    );
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(ImageKey::new(4)))
            .is_none()
    );
}

#[test]
fn oversized_atlas_image_does_not_drop_other_atlas_images() {
    let mut resources = ImageResourceStore::default();
    let small = ImageKey::new(1);
    let oversized = ImageKey::new(2);
    assert!(resources.insert(small, solid_image(1, 1, [255, 0, 0, 255])));
    assert!(resources.insert(oversized, solid_image(9, 1, [0, 255, 0, 255])));

    let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);

    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(small)),
        Some(ImageResourcePlacement::Atlas(_))
    ));
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(oversized))
            .is_none()
    );
}

#[test]
fn large_images_use_texture_table_when_enabled() {
    let key = ImageKey::new(7);
    let mut resources = ImageResourceStore::default();
    assert!(resources.insert(key, solid_image(2050, 1, [255, 0, 0, 255])));

    let upload = resources.upload_merged(
        &ImageResourceStore::default(),
        4096,
        256,
        MAX_IMAGE_RESOURCE_TEXTURES as u32,
        None,
    );

    assert_eq!(upload.textures().len(), 1);
    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(key)),
        Some(ImageResourcePlacement::Texture(texture)) if texture.index == 0 && texture.width == 2050
    ));
}

#[test]
fn large_image_texture_table_respects_actual_table_len() {
    let mut resources = ImageResourceStore::default();
    assert!(resources.insert(ImageKey::new(1), solid_image(2050, 1, [255, 0, 0, 255])));
    assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 255, 0, 255])));

    let upload = resources.upload_merged(&ImageResourceStore::default(), 4096, 4, 1, None);

    assert_eq!(upload.textures().len(), 1);
    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(ImageKey::new(1))),
        Some(ImageResourcePlacement::Texture(texture)) if texture.index == 0
    ));
    assert!(matches!(
        upload.image_placement(ImageResourceId::renderer(ImageKey::new(2))),
        Some(ImageResourcePlacement::Atlas(_))
    ));
}

#[test]
fn unchanged_upload_marks_pages_and_textures_clean() {
    let mut resources = ImageResourceStore::default();
    assert!(resources.insert(ImageKey::new(1), solid_image(4, 4, [255, 0, 0, 255])));
    assert!(resources.insert(ImageKey::new(2), solid_image(2050, 1, [0, 255, 0, 255])));
    let first = resources.upload_merged(
        &ImageResourceStore::default(),
        4096,
        256,
        MAX_IMAGE_RESOURCE_TEXTURES as u32,
        None,
    );

    let second = resources.upload_merged(
        &ImageResourceStore::default(),
        4096,
        256,
        MAX_IMAGE_RESOURCE_TEXTURES as u32,
        Some(&first),
    );

    assert!(second.atlas_pages().iter().all(|page| !page.dirty));
    assert!(second.textures().iter().all(|texture| !texture.dirty));
}

#[test]
fn updating_one_atlas_page_dirties_only_that_page() {
    let mut resources = ImageResourceStore::default();
    assert!(resources.insert(ImageKey::new(1), solid_image(4, 4, [255, 0, 0, 255])));
    assert!(resources.insert(ImageKey::new(2), solid_image(4, 4, [0, 255, 0, 255])));
    let first = resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, None);

    assert!(resources.insert(ImageKey::new(2), solid_image(4, 4, [0, 0, 255, 255])));
    let second = resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, Some(&first));

    let dirty_pages = second
        .atlas_pages()
        .iter()
        .filter(|page| page.dirty)
        .count();
    assert_eq!(dirty_pages, 1);
}

#[test]
fn updating_one_large_texture_dirties_only_that_texture() {
    let mut resources = ImageResourceStore::default();
    assert!(resources.insert(ImageKey::new(1), solid_image(2050, 1, [255, 0, 0, 255])));
    assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 255, 0, 255])));
    let first = resources.upload_merged(
        &ImageResourceStore::default(),
        4096,
        256,
        MAX_IMAGE_RESOURCE_TEXTURES as u32,
        None,
    );

    assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 0, 255, 255])));
    let second = resources.upload_merged(
        &ImageResourceStore::default(),
        4096,
        256,
        MAX_IMAGE_RESOURCE_TEXTURES as u32,
        Some(&first),
    );

    assert_eq!(second.textures().len().min(MAX_IMAGE_RESOURCE_TEXTURES), 2);
    let dirty_textures = second
        .textures()
        .iter()
        .filter(|texture| texture.dirty)
        .count();
    assert_eq!(dirty_textures, 1);
}

fn solid_image(width: u32, height: u32, color: [u8; 4]) -> Image {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        rgba.extend_from_slice(&color);
    }
    Image::from_rgba8(width, height, rgba)
}
