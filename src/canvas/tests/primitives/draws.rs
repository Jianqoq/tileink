use super::*;

#[test]
fn draw_id_updates_specific_draw_color() {
    let mut canvas = test_scene();
    let first = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let second = canvas.push_circle(Circle::new((32.0, 32.0), 8.0), Brush::Solid(rgb(0, 255, 0)));

    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(canvas.draw_id_at(0), Some(first));
    assert_eq!(canvas.draw_id_at(1), Some(second));

    assert!(canvas.set_draw_color(first, rgb(8, 9, 10)));
    assert_eq!(canvas.draw_solid_color(first), Some(rgb(8, 9, 10)));
    assert_eq!(canvas.draw_solid_color(second), Some(rgb(0, 255, 0)));
    let first_brush = canvas.draw_records[first.index()].brush_offset as usize;
    let second_brush = canvas.draw_records[second.index()].brush_offset as usize;
    assert_eq!(
        canvas.brush_blob[first_brush + 4],
        premul_color_to_rgba8_pack(rgb(8, 9, 10))
    );
    assert_eq!(
        canvas.brush_blob[second_brush + 4],
        premul_color_to_rgba8_pack(rgb(0, 255, 0))
    );
}

#[test]
fn draw_id_from_before_reset_is_rejected() {
    let mut canvas = test_scene();
    let stale = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    canvas.reset();
    let current = canvas.push_rect(
        Rect::new(8.0, 9.0, 24.0, 25.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(0, 255, 0)),
    );

    assert_eq!(stale.index(), current.index());
    assert_ne!(stale, current);
    assert!(!canvas.set_draw_color(stale, rgb(255, 255, 0)));
    assert_eq!(canvas.draw_solid_color(current), Some(rgb(0, 255, 0)));
}

#[test]
fn reset_for_surface_retargets_canvas_without_discarding_storage_capacity() {
    let mut canvas = test_scene();
    let stale = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let draw_capacity = canvas.draw_records.capacity();
    let brush_capacity = canvas.brush_blob.capacity();
    let sdf_capacity = canvas.sdf_blob.capacity();

    canvas.reset_for_surface(80, 48, 2.0);

    assert_eq!(canvas.logical_size(), (80, 48));
    assert_eq!(canvas.physical_size(), (160, 96));
    assert_eq!(canvas.scale_factor(), 2.0);
    assert_eq!(canvas.draw_count(), 0);
    assert!(canvas.is_closed_for_append());
    assert!(canvas.draw_records.capacity() >= draw_capacity);
    assert!(canvas.brush_blob.capacity() >= brush_capacity);
    assert!(canvas.sdf_blob.capacity() >= sdf_capacity);
    assert!(!canvas.set_draw_color(stale, rgb(0, 255, 0)));
}

// Regression: validating the replacement scale after clearing storage destroyed the last valid
// scene when callers supplied an invalid scale. Rejection must leave the Canvas untouched.
#[test]
fn reset_for_surface_rejects_invalid_scale_without_mutating_canvas() {
    let mut canvas = test_scene();
    let draw = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        canvas.reset_for_surface(80, 48, f32::NAN);
    }));

    assert!(result.is_err());
    assert_eq!(canvas.logical_size(), (64, 64));
    assert_eq!(canvas.scale_factor(), 1.0);
    assert_eq!(canvas.draw_solid_color(draw), Some(rgb(255, 0, 0)));
}

#[test]
fn canvas_reports_unclosed_layers_before_retained_installation() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    assert!(canvas.is_closed_for_append());
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO);
    assert!(!canvas.is_closed_for_append());
    canvas.pop_layer();
    assert!(canvas.is_closed_for_append());
}
