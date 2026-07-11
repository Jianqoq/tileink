use super::*;
use crate::{
    TextLayoutOptions,
    shared::{
        image::{Image, premul_color_to_rgba8_pack},
        image_resource::{ImageKey, ImageResourceId},
    },
};
use std::sync::Arc;

fn draw_sdf(canvas: &Canvas, index: usize) -> Option<Sdf> {
    canvas.draw_sdf(&canvas.draw_records[index])
}

fn draw_sdf_shadow(canvas: &Canvas, index: usize) -> Option<SdfShadow> {
    canvas.draw_sdf_shadow(&canvas.draw_records[index])
}

fn draw_brush(canvas: &Canvas, index: usize) -> Brush {
    canvas
        .draw_brush_for_record(&canvas.draw_records[index])
        .unwrap()
}

#[test]
fn push_rect_records_sdf_rect_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 2,
            y0: 3,
            x1: 18,
            y1: 19,
        }
    );
    assert_eq!(draw.tag, DrawTag::Brush);
    assert!(draw.path_id().is_none());
    assert!(!draw.solid_rect());
    match canvas.draw_sdf(draw) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (2.0, 3.0, 18.0, 19.0));
            assert!(rect.radius.is_zero());
        }
        sdf => panic!("expected rect SDF, got {sdf:?}"),
    }
}

#[test]
fn push_rect_records_sdf_rect_with_independent_radii() {
    let mut canvas = test_scene();
    let radius = Radius {
        top_left: 3.0,
        top_right: 9.0,
        bottom_left: 15.0,
        bottom_right: 21.0,
    };
    canvas.push_rect(
        Rect::new(4.0, 5.0, 40.0, 41.0),
        radius,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (4.0, 5.0, 40.0, 41.0));
            assert_eq!(rect.radius.top_left, 3.0);
            assert_eq!(rect.radius.top_right, 9.0);
            assert_eq!(rect.radius.bottom_left, 15.0);
            assert_eq!(rect.radius.bottom_right, 21.0);
        }
        sdf => panic!("expected rect SDF, got {sdf:?}"),
    }
}

#[test]
fn new_stores_logical_size_and_scales_sdf_rects() {
    let mut canvas = Canvas::new(21, 11, 1.5);
    assert_eq!(canvas.physical_size(), (32, 17));
    assert_eq!(canvas.scale_factor(), 1.5);

    canvas.push_rect(
        Rect::new(2.0, 3.0, 10.0, 11.0),
        Radius {
            top_left: 1.0,
            top_right: 2.0,
            bottom_left: 3.0,
            bottom_right: 4.0,
        },
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 3,
            y0: 4,
            x1: 15,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (3.0, 4.5, 15.0, 16.5));
            assert_eq!(rect.radius.top_left, 1.5);
            assert_eq!(rect.radius.top_right, 3.0);
            assert_eq!(rect.radius.bottom_left, 4.5);
            assert_eq!(rect.radius.bottom_right, 6.0);
        }
        sdf => panic!("expected scaled rect SDF, got {sdf:?}"),
    }
}

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
    let image = Arc::new(Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]));

    canvas
        .push_image(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            Arc::clone(&image),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("first image draw");
    canvas
        .push_image(
            Rect::new(2.0, 0.0, 4.0, 1.0),
            Arc::clone(&image),
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

#[test]
fn push_circle_records_sdf_circle_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle(Circle::new((16.0, 20.0), 8.0), Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 12,
            x1: 24,
            y1: 28,
        }
    );
    match canvas.draw_sdf(draw) {
        Some(Sdf::Circle(circle)) => {
            assert_eq!(circle.center, Point::new(16.0, 20.0));
            assert_eq!(circle.radius, 8.0);
        }
        sdf => panic!("expected circle SDF, got {sdf:?}"),
    }
}

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
fn no_op_sdf_primitive_returns_no_draw_id() {
    let mut canvas = test_scene();
    let draw = canvas.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(8.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(draw, None);
    assert!(canvas.draw_records.is_empty());
}

#[test]
fn scene_records_rebuild_after_append() {
    let mut parent = test_scene();
    parent.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    let mut child = test_scene();
    child.push_circle(Circle::new((16.0, 16.0), 8.0), Brush::Solid(rgb(0, 255, 0)));
    parent.append(&child, Point::new(4.0, 5.0));

    assert!(parent.lines.is_empty());
    assert!(parent.path_records.is_empty());
    assert_eq!(
        parent.sdf_blob.len(),
        2 * crate::shared::gpu_sdf::ENCODED_SDF_WORDS
    );
}

#[test]
fn append_fast_path_translates_sdf_without_mutating_child() {
    let mut child = test_scene();
    child.push_rect(
        Rect::new(1.0, 2.0, 5.0, 6.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let original_child_draw = child.draw_records[0];

    let mut parent = test_scene();
    parent.append(&child, Point::new(10.0, 20.0));
    parent.append(&child, Point::new(30.0, 40.0));

    assert_eq!(
        child.draw_records[0].pixel_bounds,
        original_child_draw.pixel_bounds
    );
    assert!(matches!(draw_sdf(&child, 0), Some(Sdf::Rect(_))));
    assert!(original_child_draw.sdf_range().is_some());
    assert_eq!(parent.draw_records.len(), 2);
    assert_eq!(
        parent.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 11,
            y0: 22,
            x1: 15,
            y1: 26,
        }
    );
    assert_eq!(
        parent.draw_records[1].pixel_bounds,
        PixelBounds {
            x0: 31,
            y0: 42,
            x1: 35,
            y1: 46,
        }
    );
}

#[test]
fn append_fast_path_offsets_text_runs_without_mutating_child() {
    let mut font_system = TextFontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("AA", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut child = test_scene();
    child
        .push_text_layout(&layout, Point::new(0.0, 20.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");
    let child_runs = child.text_runs.clone();
    let child_glyphs = child.text_glyphs.clone();

    let mut parent = test_scene();
    parent.append(&child, Point::new(4.0, 8.0));
    parent.append(&child, Point::new(40.0, 80.0));

    assert_eq!(child.text_runs.len(), child_runs.len());
    for (actual, expected) in child.text_runs.iter().zip(&child_runs) {
        assert_eq!(actual.glyph_start, expected.glyph_start);
        assert_eq!(actual.glyph_count, expected.glyph_count);
    }
    assert_eq!(child.text_glyphs.len(), child_glyphs.len());
    for (actual, expected) in child.text_glyphs.iter().zip(&child_glyphs) {
        assert_eq!(actual.x, expected.x);
        assert_eq!(actual.y, expected.y);
    }
    assert_eq!(parent.text_runs.len(), 2);
    assert_eq!(parent.text_runs[0].glyph_start, 0);
    assert_eq!(parent.text_runs[1].glyph_start, child_glyphs.len() as u32);
}

#[test]
fn text_draw_tracks_run_and_glyph_positions() {
    let mut font_system = TextFontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Cache", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut canvas = test_scene();
    let draw = canvas
        .push_text_layout(&layout, Point::new(8.0, 32.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert_eq!(canvas.draw_records[draw.index()].glyph_run_id(), Some(0));
    assert_eq!(canvas.text_runs[0].glyph_start, 0);
    assert_eq!(
        canvas.text_runs[0].glyph_count,
        canvas.text_glyphs.len() as u32
    );
}

#[test]
fn push_text_layout_scales_glyph_cache_keys_to_physical_size() {
    let mut font_system = TextFontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Scale", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut canvas = Canvas::new(128, 64, 2.0);
    canvas
        .push_text_layout(&layout, Point::new(4.0, 12.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert!(!canvas.text_glyphs.is_empty());
    let glyph = canvas.text_glyphs[0];
    assert_eq!(f32::from_bits(glyph.cache_key.font_size_bits), 40.0);
    assert!(glyph.x >= 8);
}

#[test]
fn push_candlestick_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_candlestick(
        SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7, 1),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 13,
            y0: 4,
            x1: 20,
            y1: 28,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::CandleStick(candle)) => {
            assert_eq!(candle.center_x, 16.5);
            assert_eq!(candle.body_width, 7);
            assert_eq!(candle.wick_width, 1);
        }
        sdf => panic!("expected candlestick SDF, got {sdf:?}"),
    }
}

#[test]
fn push_candlestick_accepts_even_body_and_custom_wick_width_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_candlestick(
        SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 8, 3),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 4,
            x1: 21,
            y1: 28,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::CandleStick(candle)) => {
            assert_eq!(candle.body_width, 8);
            assert_eq!(candle.wick_width, 3);
        }
        sdf => panic!("expected candlestick SDF, got {sdf:?}"),
    }
}

#[test]
fn push_line_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 25,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Line(line)) => assert_eq!(line.width, 1.0),
        sdf => panic!("expected line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_dash_line_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_dash_line(
        crate::SdfDashLine::new(
            Point::new(8.0, 16.5),
            Point::new(32.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
            4.0,
            3.0,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 33,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::DashLine(line)) => {
            assert_eq!(line.dash_length, 4.0);
            assert_eq!(line.gap_length, 3.0);
        }
        sdf => panic!("expected dash line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_sdf_arc_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_sdf_arc(
        SdfArc::new(
            Point::new(32.0, 32.0),
            12.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            4.0,
            crate::shared::sdf::line::LineCap::Round,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Arc(arc)) => {
            assert_eq!(arc.center, Point::new(32.0, 32.0));
            assert_eq!(arc.radius, 12.0);
            assert_eq!(arc.width, 4.0);
        }
        sdf => panic!("expected arc SDF, got {sdf:?}"),
    }
}

#[test]
fn push_shape_shadows_record_sdf_shadow_without_path_storage() {
    let mut canvas = test_scene();
    let options = RectShadowOptions::new(2.0, 3.0, 4.0, 0.5);
    canvas.push_circle_shadow(
        Circle::new((20.0, 20.0), 8.0),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );
    canvas.push_arc_shadow(
        SdfArc::new(
            Point::new(32.0, 32.0),
            10.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            3.0,
            crate::shared::sdf::line::LineCap::Round,
        ),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );
    canvas.push_line_shadow(
        SdfLine::new(
            Point::new(8.0, 12.0),
            Point::new(28.0, 12.0),
            2.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 3);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert!(
        canvas
            .draw_records
            .iter()
            .all(|draw| draw.sdf_range().is_none())
    );
    assert!(matches!(
        draw_sdf_shadow(&canvas, 0),
        Some(SdfShadow::Circle(_))
    ));
    assert!(matches!(
        draw_sdf_shadow(&canvas, 1),
        Some(SdfShadow::Arc(_))
    ));
    assert!(matches!(
        draw_sdf_shadow(&canvas, 2),
        Some(SdfShadow::Line(_))
    ));
}

#[test]
fn push_rect_stroke_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        Stroke::new(6.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 9,
            x1: 33,
            y1: 39,
        }
    );
    assert!(draw.path_id().is_none());
    match canvas.draw_sdf(draw) {
        Some(Sdf::RectStroke(stroke)) => {
            assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
            assert_eq!(stroke.rect.radius.top_left, 4.0);
            assert_eq!(stroke.widths, StrokeWidths::all(6.0));
        }
        sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_rect_stroke_widths_records_per_side_sdf_widths() {
    let mut canvas = test_scene();
    let widths = StrokeWidths {
        top: 2.0,
        right: 6.0,
        bottom: 10.0,
        left: 4.0,
    };
    canvas.push_rect_stroke_widths(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        widths,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 11,
            x1: 33,
            y1: 41,
        }
    );
    match canvas.draw_sdf(draw) {
        Some(Sdf::RectStroke(stroke)) => {
            assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
            assert_eq!(stroke.widths, widths);
        }
        sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_circle_stroke_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 8,
            x1: 36,
            y1: 32,
        }
    );
    match canvas.draw_sdf(draw) {
        Some(Sdf::CircleStroke(stroke)) => {
            assert_eq!(stroke.circle.center, Point::new(24.0, 20.0));
            assert_eq!(stroke.circle.radius, 10.0);
            assert_eq!(stroke.half_width, 2.0);
        }
        sdf => panic!("expected circle stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_sdf_stroke_with_zero_width_is_noop() {
    let mut canvas = test_scene();
    canvas.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::ZERO,
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert!(canvas.draw_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
}

#[test]
fn push_dashed_circle_stroke_uses_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0).with_dashes(0.0, [4.0, 4.0]),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert_eq!(canvas.path_records.len(), 1);
    assert_eq!(canvas.path_records.len(), 1);
    assert!(canvas.draw_records[0].path_id().is_some());
    assert!(canvas.draw_records[0].sdf_range().is_none());
    assert!(canvas.draw_records[0].sdf_shadow_range().is_none());
}
