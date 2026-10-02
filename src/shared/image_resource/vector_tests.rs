use super::*;
use crate::Canvas;

fn vector(width: u32, height: u32) -> ImageSource {
    ImageSource::Vector(Rc::new(Canvas::new(width, height, 1.0)))
}

#[test]
fn vector_extent_uses_canvas_physical_pixels_without_raster_storage() {
    let scene = Rc::new(Canvas::new(17, 9, 2.0));
    let source = ImageSource::Vector(scene.clone());
    assert_eq!(source.size(), (34, 18));
    assert!(source.raster().is_none());
    let mut store = ImageResourceStore::default();
    assert!(store.insert(ImageKey::new(1), source));
    let upload = store.upload_merged(&ImageResourceStore::default(), 1024, 8, 16, None);
    assert_eq!(upload.vectors().len(), 1);
    assert!(Rc::ptr_eq(&upload.vectors()[0].canvas, &scene));
    let ImageResourcePlacement::Atlas(rect) = upload.vectors()[0].placement else {
        panic!("small image uses atlas")
    };
    assert_eq!((rect.width, rect.height), (34, 18));
    assert!(rect.x > 0 && rect.y > 0);
}

#[test]
fn mixed_atlas_keeps_raster_texels_and_reserves_a_vector_border() {
    let mut store = ImageResourceStore::default();
    store.insert(
        ImageKey::new(1),
        Image {
            width: 1,
            height: 1,
            pixels: vec![0x80402010],
        },
    );
    store.insert(ImageKey::new(2), vector(2, 3));
    let upload = store.upload_merged(&ImageResourceStore::default(), 16, 1, 0, None);
    let ImageResourcePlacement::Atlas(raster) = upload
        .image_placement(ImageResourceId::renderer(ImageKey::new(1)))
        .unwrap()
    else {
        panic!("atlas")
    };
    let ImageResourcePlacement::Atlas(vector) = upload.vectors()[0].placement else {
        panic!("atlas")
    };
    let page = &upload.atlas_pages()[raster.page as usize];
    for y in raster.y - 1..=raster.y + raster.height {
        for x in raster.x - 1..=raster.x + raster.width {
            assert_eq!(page.pixels[(y * page.size + x) as usize], 0x80402010);
        }
    }
    for y in vector.y - 1..=vector.y + vector.height {
        for x in vector.x - 1..=vector.x + vector.width {
            assert_eq!(page.pixels[(y * page.size + x) as usize], 0);
        }
    }
}

#[test]
fn large_vector_uses_texture_table_without_fake_raster_bytes() {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey::new(1), vector(2048, 2));
    let upload = store.upload_merged(&ImageResourceStore::default(), 4096, 8, 16, None);
    assert_eq!(upload.textures().len(), 1);
    assert!(upload.textures()[0].pixels.is_empty());
    assert!(matches!(
        upload.vectors()[0].placement,
        ImageResourcePlacement::Texture(_)
    ));
    assert_eq!(
        (upload.textures()[0].width, upload.textures()[0].height),
        (2048, 2)
    );
}

#[test]
fn unchanged_vector_reuses_placement_but_replacement_requires_refresh() {
    let mut store = ImageResourceStore::default();
    store.insert(ImageKey::new(1), vector(17, 9));
    let empty = ImageResourceStore::default();
    let first = store.upload_merged(&empty, 1024, 8, 16, None);
    let second = store.upload_merged(&empty, 1024, 8, 16, Some(&first));
    assert!(first.vectors()[0].dirty);
    assert!(!second.vectors()[0].dirty);
    assert_eq!(first.vectors()[0].placement, second.vectors()[0].placement);
    store.insert(ImageKey::new(1), vector(17, 9));
    let replacement = store.upload_merged(&empty, 1024, 8, 16, Some(&second));
    assert!(replacement.vectors()[0].dirty);
}

#[test]
fn empty_resource_graph_drops_old_vector_dependencies() {
    let mut store = ImageResourceStore::default();
    let scene = Rc::new(Canvas::new(17, 9, 1.0));
    let weak = Rc::downgrade(&scene);
    store.insert(ImageKey::new(1), ImageSource::Vector(scene));
    let previous = store.upload_merged(&ImageResourceStore::default(), 1024, 8, 16, None);
    store.clear();
    let empty = store.upload_merged(&ImageResourceStore::default(), 1024, 8, 16, Some(&previous));
    assert!(empty.vectors().is_empty());
    assert!(empty.is_empty());
    drop(previous);
    assert!(weak.upgrade().is_none());
}

#[test]
fn vector_identity_guard_forces_copy_on_write_without_retaining_scene() {
    let mut scene = Rc::new(Canvas::new(17, 9, 1.0));
    let source = ImageSource::Vector(scene.clone());
    let old_identity = source.identity();
    let guard = source.guard();
    drop(source);
    Rc::make_mut(&mut scene).reset();
    assert_ne!(ImageSource::Vector(scene).identity(), old_identity);
    drop(guard);
}

#[test]
fn empty_vector_and_over_capacity_placement_do_not_create_phantom_uploads() {
    let mut store = ImageResourceStore::default();
    assert!(!store.insert(ImageKey::new(1), vector(0, 9)));
    store.insert(ImageKey::new(2), vector(17, 9));
    let upload = store.upload_merged(&ImageResourceStore::default(), 8, 1, 0, None);
    assert!(upload.is_empty());
    assert!(upload.vectors().is_empty());
}
