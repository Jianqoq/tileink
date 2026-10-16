use super::*;

#[test]
fn push_image_records_pattern_rect_draw() {
    let mut canvas = test_scene();
    let image = Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]);
    let draw = canvas
        .push_image(
            Rect::new(10.0, 20.0, 14.0, 22.0),
            image,
            Extend::Repeat,
            PatternSampling::Nearest,
        )
        .expect("push image draw");

    assert_eq!(draw.index(), 0);
    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    let record = &canvas.draw_records[0];
    assert_eq!(
        record.pixel_bounds,
        PixelBounds {
            x0: 10,
            y0: 20,
            x1: 14,
            y1: 22,
        }
    );
    assert!(matches!(canvas.draw_sdf(record), Some(Sdf::Rect(_))));
    let Brush::Pattern(pattern) = draw_brush(&canvas, 0) else {
        panic!("expected image pattern brush");
    };
    let ImageResourceId::Scene(image_key) = pattern.image_resource_id() else {
        panic!("expected scene image pattern");
    };
    let image = canvas
        .scene_image_resources()
        .get(image_key)
        .expect("scene image resource");
    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(pattern.sampling, PatternSampling::Nearest);
    assert_eq!(pattern.extend, Extend::Repeat);
    assert_eq!(pattern.transform, [0.25, 0.0, 0.0, 0.5, -2.5, -10.0]);
}

#[test]
fn push_image_reuses_scene_resource_for_same_arc() {
    let mut canvas = test_scene();
    let image = Rc::new(Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]));

    canvas
        .push_image(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            Rc::clone(&image),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("first image draw");
    canvas
        .push_image(
            Rect::new(2.0, 0.0, 4.0, 1.0),
            Rc::clone(&image),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("second image draw");

    let Brush::Pattern(first) = draw_brush(&canvas, 0) else {
        panic!("expected first image pattern brush");
    };
    let Brush::Pattern(second) = draw_brush(&canvas, 1) else {
        panic!("expected second image pattern brush");
    };
    assert_eq!(first.image_resource_id(), second.image_resource_id());
}

#[test]
fn new_keeps_image_brush_sampling_in_logical_coordinates() {
    let mut canvas = Canvas::new(64, 64, 2.0);
    canvas
        .push_image(
            Rect::new(10.0, 20.0, 14.0, 22.0),
            Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]),
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .expect("push image draw");

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 20,
            y0: 40,
            x1: 28,
            y1: 44,
        }
    );
    let Brush::Pattern(pattern) = draw_brush(&canvas, 0) else {
        panic!("expected image pattern brush");
    };
    assert_eq!(pattern.transform, [0.125, 0.0, 0.0, 0.25, -2.5, -10.0]);
}

#[test]
fn push_image_key_records_resource_pattern_rect_draw() {
    let mut canvas = test_scene();
    let key = ImageKey::new(42);
    let draw = canvas
        .push_image_key(
            Rect::new(10.0, 20.0, 14.0, 22.0),
            key,
            Extend::Reflect,
            PatternSampling::Nearest,
        )
        .expect("push image resource draw");

    assert_eq!(draw.index(), 0);
    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    let record = &canvas.draw_records[0];
    assert!(matches!(canvas.draw_sdf(record), Some(Sdf::Rect(_))));
    let Brush::Pattern(pattern) = draw_brush(&canvas, 0) else {
        panic!("expected image resource pattern brush");
    };
    assert_eq!(pattern.image_key(), Some(key));
    assert_eq!(pattern.sampling, PatternSampling::Nearest);
    assert_eq!(pattern.extend, Extend::Reflect);
    assert_eq!(pattern.transform, [0.25, 0.0, 0.0, 0.5, -2.5, -10.0]);
}

#[test]
fn append_translates_image_brush_without_mutating_child() {
    let mut child = test_scene();
    child
        .push_image(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push child image");
    let Brush::Pattern(original_child_pattern) = draw_brush(&child, 0) else {
        panic!("expected child image pattern brush");
    };
    let original_transform = original_child_pattern.transform;

    let mut parent = test_scene();
    parent.append(&child, Point::new(10.0, 20.0));

    let Brush::Pattern(child_pattern) = draw_brush(&child, 0) else {
        panic!("expected child image pattern brush");
    };
    assert_eq!(child_pattern.transform, original_transform);
    let Brush::Pattern(parent_pattern) = draw_brush(&parent, 0) else {
        panic!("expected parent image pattern brush");
    };
    assert_eq!(parent_pattern.transform, [0.5, 0.0, 0.0, 1.0, -5.0, -20.0]);
    assert_eq!(
        parent.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 10,
            y0: 20,
            x1: 12,
            y1: 21,
        }
    );
}

#[test]
fn push_image_rejects_empty_images_and_rects() {
    let mut canvas = test_scene();

    assert!(
        canvas
            .push_image(
                Rect::new(0.0, 0.0, 4.0, 4.0),
                Image::from_rgba8(0, 1, []),
                Extend::Pad,
                PatternSampling::Bilinear,
            )
            .is_none()
    );
    assert!(
        canvas
            .push_image(
                Rect::new(0.0, 0.0, 0.0, 4.0),
                Image::from_rgba8(1, 1, [255, 0, 0, 255]),
                Extend::Pad,
                PatternSampling::Bilinear,
            )
            .is_none()
    );
    assert!(canvas.draw_records.is_empty());
}
