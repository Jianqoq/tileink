//! Public store inputs must preserve namespaces, device limits and complete placements.
use super::*;

#[test]
fn renderer_and_scene_keys_keep_independent_pixels_across_empty_transitions() {
    let key = ImageKey::new(7);
    let mut renderer = ImageResourceStore::default();
    let mut scene = ImageResourceStore::default();
    renderer.insert(key, Image::from_rgba8(1, 1, [255, 0, 0, 255]));
    scene.insert(key, Image::from_rgba8(1, 1, [0, 0, 255, 255]));
    let mut previous = None;
    for expected in [
        vec![(false, 0xff00_00ff), (true, 0xffff_0000)],
        vec![(true, 0xffff_0000)],
        vec![],
    ] {
        let upload = renderer.upload_merged(&scene, 7, 4, 0, previous.as_ref());
        assert_eq!(
            [ImageResourceId::renderer(key), ImageResourceId::scene(key)]
                .into_iter()
                .filter(|&id| upload.image_placement(id).is_some())
                .count(),
            expected.len()
        );
        for (scene, pixel) in expected {
            let id = if scene {
                ImageResourceId::scene(key)
            } else {
                ImageResourceId::renderer(key)
            };
            let ImageResourcePlacement::Atlas(rect) = upload.image_placement(id).unwrap() else {
                panic!("small image uses atlas")
            };
            let page = &upload.atlas_pages()[rect.page as usize];
            assert_eq!(page.pixels[(rect.y * page.size + rect.x) as usize], pixel);
        }
        assert!(!upload.atlas_pages().is_empty());
        assert_eq!(upload.atlas_page_size(), upload.atlas_pages()[0].size);
        if renderer.contains(key) {
            renderer.clear();
        } else {
            scene.clear();
        }
        previous = Some(upload);
    }
}

#[test]
fn selected_images_fit_non_power_of_two_pages_and_capacity_boundaries() {
    for (dimension, one_page_count) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 1),
        (7, 2),
        (8, 2),
        (9, 2),
        (31, 2),
    ] {
        let size: u32 = dimension.max(1);
        let mut store = ImageResourceStore::default();
        for (index, (width, height)) in [(1, 1), (size.saturating_sub(2).max(1), 1), (size, 1)]
            .into_iter()
            .enumerate()
        {
            store.insert(
                ImageKey::new(index as u64),
                Image {
                    width,
                    height,
                    pixels: vec![index as u32 + 1; (width * height) as usize],
                },
            );
        }
        for max_pages in [0, 1, 4] {
            let upload = store.upload_merged(
                &ImageResourceStore::default(),
                dimension,
                max_pages,
                0,
                None,
            );
            assert!(!upload.atlas_pages().is_empty());
            assert_eq!(upload.atlas_page_size(), upload.atlas_pages()[0].size);
            assert!(upload.atlas_pages().len() <= (max_pages as usize).max(1));
            let mut actual_count = 0;
            for (key, _) in store.iter() {
                let Some(placement) = upload.image_placement(ImageResourceId::renderer(key)) else {
                    continue;
                };
                actual_count += 1;
                let ImageResourcePlacement::Atlas(rect) = placement else {
                    panic!("texture table disabled")
                };
                let page = &upload.atlas_pages()[rect.page as usize];
                assert_eq!(page.size, size);
                assert!(rect.x + rect.width < page.size && rect.y + rect.height < page.size);
                assert_eq!(
                    page.pixels[(rect.y * page.size + rect.x) as usize],
                    key.0 as u32 + 1
                );
            }
            let expected = match max_pages {
                0 => 0,
                1 => one_page_count,
                _ => {
                    if dimension >= 3 {
                        2
                    } else {
                        0
                    }
                }
            };
            assert_eq!(
                actual_count, expected,
                "dimension={dimension}, max_pages={max_pages}"
            );
        }
    }
}

#[test]
fn texture_selection_accepts_exact_limit_and_rejects_oversized_images() {
    let mut store = ImageResourceStore::default();
    for width in [2048, 2049, 2050, 2051] {
        store.insert(
            ImageKey::new(u64::from(width)),
            Image {
                width,
                height: 1,
                pixels: vec![width; width as usize],
            },
        );
    }
    let upload = store.upload_merged(&ImageResourceStore::default(), 2050, 0, 16, None);
    assert_eq!(upload.textures().len(), 3);
    for (index, texture) in upload.textures().iter().enumerate() {
        assert_eq!(texture.index as usize, index);
        assert_eq!(texture.width, 2048 + index as u32);
        assert!(texture.pixels.iter().all(|&pixel| pixel == texture.width));
    }
    assert!(
        upload
            .image_placement(ImageResourceId::renderer(ImageKey::new(2051)))
            .is_none()
    );
    assert_eq!(upload.atlas_page_size(), 1);
    assert_eq!(upload.atlas_pages().len(), 1);
}
