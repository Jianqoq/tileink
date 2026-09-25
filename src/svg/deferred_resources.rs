//! CPU-only lowering contracts: every SVG vector image remains a subscene.
use crate::{Canvas, shared::image_resource::ImageSource};
use std::rc::Rc;

fn lower(svg: &str) -> Canvas {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
    let mut canvas = Canvas::new(35, 19, 1.0);
    canvas.push_svg(&tree).unwrap();
    canvas
}

fn only_vector(canvas: &Canvas) -> &Rc<Canvas> {
    let mut images = canvas.scene_images.iter();
    let (_, ImageSource::Vector(child)) = images.next().expect("deferred resource") else {
        panic!("SVG vectors must not be eagerly rasterized");
    };
    assert!(images.next().is_none());
    child
}

#[test]
fn nested_svg_image_preserves_requested_raster_extent_as_vector_scene() {
    let canvas = lower(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <image width="17" height="9" preserveAspectRatio="none" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1' height='1'%3E%3Crect width='1' height='1' fill='red'/%3E%3C/svg%3E"/>
        </svg>"#,
    );
    assert_eq!(only_vector(&canvas).physical_size(), (17, 9));
}

#[test]
fn fractional_pattern_keeps_ceil_extent_without_a_gpu_device() {
    let canvas = lower(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <defs><pattern id="p" width="4.5" height="3.25" patternUnits="userSpaceOnUse">
            <rect width="2" height="2" fill="red"/>
        </pattern></defs><rect width="35" height="19" fill="url(#p)"/>
        </svg>"##,
    );
    assert_eq!(only_vector(&canvas).physical_size(), (5, 4));
}

#[test]
fn fe_image_is_deferred_in_filter_buffer_coordinates() {
    let canvas = lower(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <defs><rect id="src" width="4" height="4" fill="red"/>
        <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="17" height="9">
            <feImage href="#src" x="3" y="2" width="4" height="4"/>
        </filter></defs><rect width="17" height="9" filter="url(#f)"/>
        </svg>"##,
    );
    let child = only_vector(&canvas);
    assert_eq!(child.physical_size(), (17, 9));
    let [draw] = child.draw_records.as_slice() else {
        panic!("expected the single referenced rectangle");
    };
    let bounds = draw.pixel_bounds;
    assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (3, 2, 7, 6));
}

#[test]
fn cloned_canvases_share_immutable_subscenes_and_reset_drops_their_resources() {
    let mut canvas = lower(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <defs><pattern id="p" width="4" height="3" patternUnits="userSpaceOnUse">
            <rect width="2" height="2" fill="red"/>
        </pattern></defs><rect width="35" height="19" fill="url(#p)"/>
        </svg>"##,
    );
    let clone = canvas.clone();
    assert!(Rc::ptr_eq(only_vector(&canvas), only_vector(&clone)));
    canvas.reset();
    assert_eq!(canvas.scene_images.iter().count(), 0);
    assert_eq!(only_vector(&clone).physical_size(), (4, 3));
}
