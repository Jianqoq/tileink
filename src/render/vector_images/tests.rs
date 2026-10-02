use super::*;
use crate::shared::image_resource::{AtlasRect, TextureRect};
use std::cell::Cell;

#[test]
fn atlas_copies_duplicate_all_edges_and_corners_without_neighbor_bleed() {
    for (width, height) in [(1, 1), (1, 5), (4, 1), (3, 2), (5, 7)] {
        let rect = AtlasRect {
            page: 1,
            x: 3,
            y: 4,
            width,
            height,
        };
        let source: Vec<_> = (0..width * height).map(|i| i + 1000).collect();
        let mut destination = vec![0xdead_beef; 16 * 16 * 3];
        for copy in image_copy_regions(ImageResourcePlacement::Atlas(rect)) {
            for y in 0..copy.extent[1] {
                for x in 0..copy.extent[0] {
                    let src = ((copy.source[1] + y) * width + copy.source[0] + x) as usize;
                    let dst = (copy.destination[2] * 256
                        + (copy.destination[1] + y) * 16
                        + copy.destination[0]
                        + x) as usize;
                    destination[dst] = source[src];
                }
            }
        }
        for page in 0..3 {
            for y in 0..16 {
                for x in 0..16 {
                    let expected = if page == rect.page
                        && x >= rect.x - 1
                        && x <= rect.x + width
                        && y >= rect.y - 1
                        && y <= rect.y + height
                    {
                        let sx = x.saturating_sub(rect.x).min(width - 1);
                        let sy = y.saturating_sub(rect.y).min(height - 1);
                        source[(sy * width + sx) as usize]
                    } else {
                        0xdead_beef
                    };
                    assert_eq!(
                        destination[(page * 256 + y * 16 + x) as usize],
                        expected,
                        "{width}x{height} at layer {page}, {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn standalone_textures_copy_once_without_an_atlas_border() {
    let copies: Vec<_> = image_copy_regions(ImageResourcePlacement::Texture(TextureRect {
        index: 5,
        width: 17,
        height: 9,
    }))
    .collect();
    assert_eq!(
        copies,
        vec![ImageCopyRegion {
            source: [0, 0],
            destination: [0, 0, 0],
            extent: [17, 9]
        }]
    );
}

#[test]
#[should_panic(expected = "nonempty")]
fn zero_sized_placement_is_rejected() {
    let _ = image_copy_regions(ImageResourcePlacement::Texture(TextureRect {
        index: 0,
        width: 0,
        height: 2,
    }));
}

#[test]
fn shared_vector_sources_reuse_one_cached_renderer() {
    let source = Rc::new(Canvas::new(3, 2, 1.0));
    let mut cache = VectorImageCache::default();
    assert_eq!(cache.get_or_insert(&source, || 7).value, 7);
    assert_eq!(
        cache
            .get_or_insert(&source.clone(), || panic!("must reuse"))
            .value,
        7
    );
    assert_eq!(cache.entries.len(), 1);
}

#[test]
fn detached_vectors_are_evicted_even_while_external_owners_survive() {
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let source = Rc::new(Canvas::new(3, 2, 1.0));
    let drops = Rc::new(Cell::new(0));
    let mut cache = VectorImageCache::default();
    cache.get_or_insert(&source, || Owner(drops.clone()));
    cache.retain_sources(std::iter::empty());
    assert_eq!(drops.get(), 1);
    assert_eq!(Rc::strong_count(&source), 1);
    assert!(cache.entries.is_empty());
}

#[test]
fn retaining_a_new_graph_drops_only_its_detached_sources() {
    let first = Rc::new(Canvas::new(3, 2, 1.0));
    let second = Rc::new(Canvas::new(3, 2, 1.0));
    let mut cache = VectorImageCache::default();
    cache.get_or_insert(&first, || 1);
    cache.get_or_insert(&second, || 2);
    cache.retain_sources([&second]);
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(
        cache.get_or_insert(&second, || panic!("must reuse")).value,
        2
    );
    assert_eq!(cache.get_or_insert(&first, || 3).value, 3);
}

#[test]
fn weak_identity_guard_prevents_editing_cached_canvas_in_place() {
    let mut source = Rc::new(Canvas::new(3, 2, 1.0));
    let mut cache = VectorImageCache::default();
    cache.get_or_insert(&source, || 1);
    let old = Rc::as_ptr(&source);
    Rc::make_mut(&mut source).reset();
    assert_ne!(old, Rc::as_ptr(&source));
    assert_eq!(cache.get_or_insert(&source, || 2).value, 2);
}
