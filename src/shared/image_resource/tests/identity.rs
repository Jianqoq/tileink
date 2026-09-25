use std::rc::Rc;

use super::{ImageKey, ImageResourceId, ImageResourcePlacement, ImageResourceStore};
use crate::shared::image::Image;

#[test]
fn changed_image_after_source_release_invalidates_cached_upload() {
    let red = u32::from_le_bytes([255, 0, 0, 255]);
    let blue = u32::from_le_bytes([0, 0, 255, 255]);
    for (width, texture_table_len) in [(4, 0), (2050, 1)] {
        for scene_image in [false, true] {
            let key = ImageKey::new(7);
            let empty = ImageResourceStore::default();
            let mut source = ImageResourceStore::default();
            let mut image = Rc::new(Image {
                width,
                height: 1,
                pixels: vec![red; width as usize],
            });
            source.insert(key, image.clone());
            let (renderer, scene) = if scene_image {
                (&empty, &source)
            } else {
                (&source, &empty)
            };
            let before_signature = renderer.upload_signature(scene, 4096, 1, texture_table_len);
            let before = renderer.upload_merged(scene, 4096, 1, texture_table_len, None);
            drop(source);

            // Without a live identity guard, this edits the same Rc and pixel allocations.
            // It deterministically reproduces address reuse, without depending on an allocator.
            Rc::make_mut(&mut image).pixels.fill(blue);
            let mut replacement = ImageResourceStore::default();
            replacement.insert(key, image);
            let (renderer, scene) = if scene_image {
                (&empty, &replacement)
            } else {
                (&replacement, &empty)
            };
            let after_signature = renderer.upload_signature(scene, 4096, 1, texture_table_len);
            assert_ne!(
                before_signature, after_signature,
                "a changed image must invalidate preparation"
            );
            let after = renderer.upload_merged(scene, 4096, 1, texture_table_len, Some(&before));
            let id = if scene_image {
                ImageResourceId::scene(key)
            } else {
                ImageResourceId::renderer(key)
            };
            match after.image_placement(id).unwrap() {
                ImageResourcePlacement::Atlas(rect) => {
                    let page = &after.atlas_pages()[rect.page as usize];
                    assert!(page.dirty);
                    for x in 0..width {
                        assert_eq!(
                            page.pixels[(rect.y * page.size + rect.x + x) as usize],
                            blue
                        );
                    }
                }
                ImageResourcePlacement::Texture(rect) => {
                    let texture = &after.textures()[rect.index as usize];
                    assert!(texture.dirty);
                    assert!(texture.pixels.iter().all(|pixel| *pixel == blue));
                }
            }
        }
    }
}

#[test]
fn cached_image_identity_does_not_retain_source_pixels() {
    let image = Rc::new(Image::from_rgba8(1, 1, [255, 0, 0, 255]));
    let lifetime = Rc::downgrade(&image);
    let mut resources = ImageResourceStore::default();
    resources.insert(ImageKey::new(1), image.clone());
    let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 1, 0, None);
    let cached_clone = upload.clone();
    drop(upload);
    drop(resources);
    drop(image);
    assert!(
        lifetime.upgrade().is_none(),
        "cached identities must not keep pixel buffers alive"
    );
    assert!(!cached_clone.atlas_pages().is_empty());
}
