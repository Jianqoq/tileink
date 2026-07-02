use peniko::{
    Color, Compose, Gradient, Mix,
    kurbo::{Affine, Circle, Line, Point, Rect, RoundedRect, Shape, Stroke},
};

use super::Renderer;
use crate::{
    Brush, CandleStick, FillRule, Radius, RectLiquidGlass, RectShadowOptions, Scene, SdfArc,
    SdfDashLine, SdfLine, SdfLineCap, StrokeWidths, TextContext, TextLayoutOptions,
    shared::layer::{
        filter::Filter,
        mask::{Mask, MaskKind},
        region::Region,
    },
};

fn render_single_rounded_rect() -> Renderer {
    let mut scene = Scene::new(320, 240);
    scene.push_path(
        RoundedRect::new(48.0, 48.0, 280.0, 200.0, (36.0, 36.0, 8.0, 8.0)).to_path(0.1),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    let mut renderer = Renderer::new(320, 240, Color::WHITE);
    renderer.render(&scene);
    renderer
}

fn assert_rgb_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
    for channel in 0..4 {
        let delta = actual[channel].abs_diff(expected[channel]);
        assert!(
            delta <= tolerance,
            "channel {channel} expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn render_with_text_rasterizes_scene_text_layout() {
    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(128, 64);
    scene.push_text_layout(&layout, Point::new(8.0, 32.0), Color::BLACK);

    let mut renderer = Renderer::new(128, 64, Color::WHITE);
    renderer.render_with_text(&scene, &mut text_context);

    let has_text_pixel = (0..64).any(|y| {
        (0..128).any(|x| {
            let [r, g, b, a] = renderer.image().rgba8_at(x, y);
            a == 255 && (r < 250 || g < 250 || b < 250)
        })
    });
    assert!(has_text_pixel, "expected text to darken at least one pixel");
}

#[test]
fn render_with_text_rasterizes_emoji_or_fallback_glyphs() {
    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Emoji 😀 👍🏽", 32.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(192, 64);
    scene.push_text_layout(&layout, Point::new(8.0, 42.0), Color::BLACK);

    let mut renderer = Renderer::new(192, 64, Color::WHITE);
    renderer.render_with_text(&scene, &mut text_context);

    let has_non_background_pixel = (0..64).any(|y| {
        (0..192).any(|x| {
            let [r, g, b, a] = renderer.image().rgba8_at(x, y);
            a == 255 && [r, g, b] != [255, 255, 255]
        })
    });
    assert!(
        has_non_background_pixel,
        "expected emoji text or fallback glyphs to render"
    );
}

#[test]
fn render_path_text_rasterizes_vector_outlines_without_text_atlas() {
    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Path", 40.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(160, 72);
    scene.push_text_layout_as_path(
        &mut text_context,
        &layout,
        Point::new(8.0, 52.0),
        Color::BLACK,
        Affine::IDENTITY,
        0.1,
    );

    let mut renderer = Renderer::new(160, 72, Color::WHITE);
    renderer.render(&scene);

    let has_text_pixel = (0..72).any(|y| {
        (0..160).any(|x| {
            let [r, g, b, a] = renderer.image().rgba8_at(x, y);
            a == 255 && (r < 250 || g < 250 || b < 250)
        })
    });
    assert!(
        has_text_pixel,
        "expected outline text to darken at least one pixel"
    );
}

#[test]
fn rounded_rect_top_right_keeps_inside_filled() {
    let renderer = render_single_rounded_rect();

    assert_eq!(renderer.image().rgba8_at(260, 60), [37, 99, 235, 255]);
}

#[test]
fn rounded_rect_top_right_keeps_outside_empty() {
    let renderer = render_single_rounded_rect();

    assert_eq!(renderer.image().rgba8_at(276, 52), [255, 255, 255, 255]);
}

#[test]
fn plain_rect_right_edge_keeps_inside_filled() {
    let mut scene = Scene::new(320, 240);
    scene.push_path(
        Rect::new(48.0, 48.0, 280.0, 200.0).to_path(0.0),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut renderer = Renderer::new(320, 240, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(279, 60), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(280, 60), [255, 255, 255, 255]);
}

#[test]
fn append_places_path_scene_at_position() {
    let mut child = Scene::new(16, 16);
    child.push_path(
        Rect::new(0.0, 0.0, 12.0, 12.0).to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut scene = Scene::new(40, 32);
    scene.append(child, (17.0, 5.0));

    let mut renderer = Renderer::new(40, 32, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(16, 5), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(17, 5), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(28, 16), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(29, 16), [255, 255, 255, 255]);
}

#[test]
fn append_does_not_clip_to_child_scene_canvas() {
    let mut child = Scene::new(16, 16);
    child.push_rect(
        Rect::new(8.0, 8.0, 24.0, 24.0),
        Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );

    let mut scene = Scene::new(40, 40);
    scene.append(child, (10.0, 10.0));

    let mut renderer = Renderer::new(40, 40, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(17, 18), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(18, 18), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(33, 33), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(34, 33), [255, 255, 255, 255]);
}

#[test]
fn append_inside_open_layer_stays_inside_that_layer() {
    let mut child = Scene::new(16, 16);
    child.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );

    let mut scene = Scene::new(40, 32);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 20.0, 32.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.append(child, (10.0, 8.0));
    scene.pop_layer();

    let mut renderer = Renderer::new(40, 32, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(19, 10), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(20, 10), [255, 255, 255, 255]);
}

#[test]
fn append_moves_linear_gradient_with_child_scene() {
    let gradient = Gradient::new_linear((0.0, 0.0), (16.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut child = Scene::new(16, 4);
    child.push_rect(
        Rect::new(0.0, 0.0, 16.0, 4.0),
        Radius::ZERO,
        Brush::from_gradient(&gradient),
    );

    let mut scene = Scene::new(40, 4);
    scene.append(child, (20.0, 0.0));

    let mut renderer = Renderer::new(40, 4, Color::WHITE);
    renderer.render(&scene);

    let left = renderer.image().rgba8_at(21, 2);
    let right = renderer.image().rgba8_at(35, 2);
    assert!(left[0] > left[2], "left should stay red-biased: {left:?}");
    assert!(
        right[2] > right[0],
        "right should stay blue-biased: {right:?}"
    );
}

#[test]
fn clip_layer_masks_child_fill() {
    let mut scene = Scene::new(96, 96);
    scene.push_clip_layer(
        Rect::new(24.0, 24.0, 72.0, 72.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_path(
        Rect::new(8.0, 8.0, 88.0, 88.0).to_path(0.0),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 48), [255, 255, 255, 255]);
}

#[test]
fn sdf_rect_clip_masks_child_fill() {
    let mut scene = Scene::new(96, 96);
    scene.push_clip_sdf_rect_layer(Rect::new(24.0, 24.0, 72.0, 72.0), Radius::ZERO);
    scene.push_rect(
        Rect::new(8.0, 8.0, 88.0, 88.0),
        crate::Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 48), [255, 255, 255, 255]);
}

#[test]
fn sdf_circle_clip_masks_child_fill() {
    let mut scene = Scene::new(96, 96);
    scene.push_clip_sdf_circle_layer(Circle::new((48.0, 48.0), 24.0));
    scene.push_rect(
        Rect::new(0.0, 0.0, 96.0, 96.0),
        crate::Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 48), [255, 255, 255, 255]);
}

#[test]
fn sdf_rect_clip_keeps_subpixel_edge_coverage() {
    let mut scene = Scene::new(48, 48);
    scene.push_clip_sdf_rect_layer(Rect::new(16.25, 8.0, 32.25, 40.0), Radius::ZERO);
    scene.push_rect(
        Rect::new(0.0, 0.0, 48.0, 48.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(48, 48, Color::WHITE);
    renderer.render(&scene);

    let edge = renderer.image().rgba8_at(16, 24);
    assert_eq!(renderer.image().rgba8_at(15, 24), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(17, 24), [255, 0, 0, 255]);
    assert_eq!(edge[0], 255);
    assert_eq!(edge[1], edge[2]);
    assert!(
        edge[1] > 0 && edge[1] < 255,
        "expected partially covered edge pixel, got {edge:?}"
    );
}

#[test]
fn sdf_rounded_rect_clip_masks_corners() {
    let mut scene = Scene::new(96, 96);
    scene.push_clip_sdf_rect_layer(Rect::new(16.0, 16.0, 80.0, 80.0), Radius::all(16.0));
    scene.push_rect(
        Rect::new(0.0, 0.0, 96.0, 96.0),
        crate::Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(17, 17), [255, 255, 255, 255]);
}

#[test]
fn outer_sdf_clip_applies_to_filter_output_analytically() {
    let mut scene = Scene::new(16, 16);
    scene.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 8.0, 16.0), Radius::ZERO);
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn sdf_rect_stroke_renders_ring_without_filling_center() {
    let mut scene = Scene::new(64, 64);
    scene.push_rect_stroke(
        Rect::new(16.0, 16.0, 48.0, 48.0),
        Radius::ZERO,
        Stroke::new(6.0),
        Color::from_rgb8(255, 0, 0),
    );

    let mut renderer = Renderer::new(64, 64, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(16, 32), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(8, 32), [255, 255, 255, 255]);
}

#[test]
fn sdf_candlestick_renders_centered_one_pixel_wick_and_odd_body() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_candlestick(CandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7), red);

    let mut renderer = Renderer::new(40, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(16, 5), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(14, 5), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(13, 12), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(19, 12), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(21, 12), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 29), [255, 255, 255, 255]);
}

#[test]
fn sdf_rect_shadow_renders_soft_offset_shadow_as_separate_draw() {
    let mut scene = Scene::new(72, 56);
    scene.push_rect_shadow(
        Rect::new(16.0, 12.0, 40.0, 36.0),
        Radius::all(4.0),
        RectShadowOptions::new(4.0, 4.0, 4.0, 0.5),
        Color::BLACK,
    );
    scene.push_rect(
        Rect::new(16.0, 12.0, 40.0, 36.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 64, 72),
    );

    let mut renderer = Renderer::new(72, 56, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(24, 24), [220, 64, 72, 255]);
    let near_shadow = renderer.image().rgba8_at(46, 28);
    let far_shadow = renderer.image().rgba8_at(64, 28);
    assert!(
        near_shadow[0] < 220,
        "expected soft shadow outside rect, got {near_shadow:?}"
    );
    assert!(
        far_shadow[0] > 245,
        "expected finite shadow bounds/falloff to return to background, got {far_shadow:?}"
    );
}

#[test]
fn sdf_line_renders_centered_one_pixel_butt_stroke() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            SdfLineCap::Butt,
        ),
        red,
    );

    let mut renderer = Renderer::new(40, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(23, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(7, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(24, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 15), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 17), [255, 255, 255, 255]);
}

#[test]
fn sdf_dash_line_renders_dashes_and_gaps() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(48, 36);
    scene.push_dash_line(
        SdfDashLine::new(
            Point::new(8.0, 16.5),
            Point::new(32.0, 16.5),
            1.0,
            SdfLineCap::Butt,
            4.0,
            3.0,
        ),
        red,
    );

    let mut renderer = Renderer::new(48, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(11, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(14, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(15, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 15), [255, 255, 255, 255]);
}

#[test]
fn sdf_dash_line_with_zero_gap_renders_as_solid_line() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_dash_line(
        SdfDashLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            SdfLineCap::Butt,
            4.0,
            0.0,
        ),
        red,
    );

    let mut renderer = Renderer::new(40, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(23, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(7, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(24, 16), [255, 255, 255, 255]);
}

#[test]
fn solid_horizontal_path_stroke_at_tile_top_stays_thin() {
    let mut scene = Scene::new(300, 300);
    scene.push_stroke(
        Line::new((20.2, 96.5), (180.5, 96.5)).to_path(0.25),
        Stroke::new(1.0),
        Color::from_rgb8(0, 128, 0),
        Affine::scale(1.5),
        FillRule::NonZero,
        0.25,
    );

    let mut renderer = Renderer::new(300, 300, Color::TRANSPARENT);
    renderer.render(&scene);

    assert!(renderer.image().rgba8_at(48, 144)[3] > 0);
    assert!(renderer.image().rgba8_at(48, 145)[3] > 0);
    assert_eq!(renderer.image().rgba8_at(48, 146), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(260, 146), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(48, 159), [0, 0, 0, 0]);
}

#[test]
fn dashed_path_stroke_does_not_fill_whole_tiles_at_integer_edges() {
    let chart_x = 387.0;
    let chart_y = 104.0;
    let chart_width = 652.0;
    let chart_height = 515.0;
    let close_y = 216.5;
    let mut scene = Scene::new(1071, 651);
    scene.push_rect(
        Rect::new(0.0, 0.0, 1071.0, 651.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(
            chart_x,
            chart_y,
            chart_x + chart_width,
            chart_y + chart_height,
        ),
        Radius::ZERO,
        Color::from_rgb8(244, 245, 247),
    );
    scene.push_stroke(
        Line::new((0.0, close_y), (chart_width, close_y)).to_path(0.25),
        Stroke::new(1.0).with_dashes(0.0, [1.0_f64, 2.0_f64]),
        Color::BLACK,
        Affine::translate((chart_x, chart_y)),
        FillRule::NonZero,
        0.25,
    );

    let mut renderer = Renderer::new(1071, 651, Color::WHITE);
    renderer.render(&scene);

    let y0 = (chart_y + close_y).floor() as u32;
    let mut dark_pixels = 0u32;
    for y in y0..y0 + crate::TILE_SIZE {
        let mut max_dark_run = 0u32;
        let mut dark_run = 0u32;
        for x in chart_x as u32..(chart_x + chart_width) as u32 {
            let [r, g, b, _] = renderer.image().rgba8_at(x, y);
            if r < 32 && g < 32 && b < 32 {
                dark_pixels += 1;
                dark_run += 1;
                max_dark_run = max_dark_run.max(dark_run);
            } else {
                dark_run = 0;
            }
        }

        assert!(
            max_dark_run <= 2,
            "expected 1px dash runs at y={y}, found dark run of {max_dark_run}px"
        );
    }
    assert!(dark_pixels > 0, "expected dashed path stroke to render");
}

#[test]
fn solid_crosshair_path_stroke_does_not_fill_tiles_below_horizontal_line() {
    let chart_x = 645.0;
    let chart_y = 104.0;
    let chart_width = 1450.0;
    let chart_height = 515.0;
    let crosshair_x = 487.0;
    let crosshair_y = 145.5;
    let mut scene = Scene::new(2128, 651);
    scene.push_rect(
        Rect::new(0.0, 0.0, 2128.0, 651.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(
            chart_x,
            chart_y,
            chart_x + chart_width,
            chart_y + chart_height,
        ),
        Radius::ZERO,
        Color::from_rgb8(244, 245, 247),
    );

    let transform = Affine::translate((chart_x, chart_y));
    let stroke = Stroke::new(1.0);
    let brush = Color::from_rgb8(0, 128, 255);
    scene.push_stroke(
        Line::new((crosshair_x, 0.0), (crosshair_x, chart_height)),
        stroke.clone(),
        brush,
        transform,
        FillRule::NonZero,
        0.25,
    );
    scene.push_stroke(
        Line::new((0.0, crosshair_y), (chart_width, crosshair_y)),
        stroke,
        brush,
        transform,
        FillRule::NonZero,
        0.25,
    );

    let mut renderer = Renderer::new(2128, 651, Color::WHITE);
    renderer.render(&scene);

    let y0 = (chart_y + crosshair_y).floor() as u32;
    let vx = (chart_x + crosshair_x).round() as u32;
    for y in y0 + 2..y0 + crate::TILE_SIZE {
        let mut max_blue_run = 0u32;
        let mut blue_run = 0u32;
        for x in chart_x as u32..(chart_x + chart_width) as u32 {
            if x.abs_diff(vx) <= 1 {
                blue_run = 0;
                continue;
            }

            let [r, g, b, _] = renderer.image().rgba8_at(x, y);
            if r < 16 && (96..=160).contains(&g) && b > 200 {
                blue_run += 1;
                max_blue_run = max_blue_run.max(blue_run);
            } else {
                blue_run = 0;
            }
        }

        assert_eq!(
            max_blue_run, 0,
            "horizontal crosshair leaked blue pixels below the stroke at y={y}"
        );
    }
}

#[test]
fn sdf_line_square_cap_extends_by_half_width() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            2.0,
            SdfLineCap::Square,
        ),
        red,
    );

    let mut renderer = Renderer::new(40, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(7, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(24, 16), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(6, 16), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(25, 16), [255, 255, 255, 255]);
}

#[test]
fn sdf_line_shadow_renders_soft_offset_shadow_as_separate_draw() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(48, 36);
    let line = SdfLine::new(
        Point::new(8.0, 16.5),
        Point::new(24.0, 16.5),
        1.0,
        SdfLineCap::Butt,
    );
    scene.push_line_shadow(
        line,
        RectShadowOptions::new(0.0, 4.0, 4.0, 0.5),
        Color::BLACK,
    );
    scene.push_line(line, red);

    let mut renderer = Renderer::new(48, 36, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(16, 16), [220, 64, 72, 255]);
    let near_shadow = renderer.image().rgba8_at(16, 21);
    assert!(
        near_shadow[0] < 245,
        "expected line shadow below line, got {near_shadow:?}"
    );
    let far_shadow = renderer.image().rgba8_at(16, 32);
    assert!(
        far_shadow[0] > 245,
        "expected shadow falloff to approach background, got {far_shadow:?}"
    );
}

#[test]
fn sdf_circle_shadow_renders_soft_offset_shadow_as_separate_draw() {
    let blue = Color::from_rgb8(0, 128, 255);
    let mut scene = Scene::new(72, 56);
    let circle = Circle::new((32.0, 28.0), 10.0);
    scene.push_circle_shadow(
        circle,
        RectShadowOptions::new(4.0, 4.0, 4.0, 0.5),
        Color::BLACK,
    );
    scene.push_circle(circle, blue);

    let mut renderer = Renderer::new(72, 56, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(32, 28), [0, 128, 255, 255]);
    let near_shadow = renderer.image().rgba8_at(47, 32);
    assert!(
        near_shadow[0] < 245,
        "expected circle shadow outside circle, got {near_shadow:?}"
    );
    assert_eq!(renderer.image().rgba8_at(64, 32), [255, 255, 255, 255]);
}

#[test]
fn sdf_rect_stroke_supports_per_side_widths() {
    let mut scene = Scene::new(64, 64);
    scene.push_rect_stroke_widths(
        Rect::new(20.0, 20.0, 44.0, 44.0),
        Radius::ZERO,
        StrokeWidths {
            top: 2.0,
            right: 8.0,
            bottom: 4.0,
            left: 12.0,
        },
        Color::from_rgb8(255, 0, 0),
    );

    let mut renderer = Renderer::new(64, 64, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(16, 32), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(46, 32), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 20), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 44), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(12, 32), [255, 255, 255, 255]);
}

#[test]
fn sdf_circle_stroke_renders_ring_without_filling_center() {
    let mut scene = Scene::new(64, 64);
    scene.push_circle_stroke(
        Circle::new((32.0, 32.0), 14.0),
        Stroke::new(6.0),
        Color::from_rgb8(0, 128, 255),
    );

    let mut renderer = Renderer::new(64, 64, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(18, 32), [0, 128, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(10, 32), [255, 255, 255, 255]);
}

#[test]
fn sdf_arc_renders_stroked_quarter_arc_without_flattening() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(64, 64);
    scene.push_sdf_arc(
        SdfArc::new(
            Point::new(32.0, 32.0),
            12.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            4.0,
            SdfLineCap::Round,
        ),
        red,
    );

    let mut renderer = Renderer::new(64, 64, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(40, 40), [220, 64, 72, 255]);
    assert_eq!(renderer.image().rgba8_at(24, 32), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(32, 24), [255, 255, 255, 255]);
}

#[test]
fn sdf_arc_shadow_renders_soft_offset_shadow_as_separate_draw() {
    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(72, 64);
    let arc = SdfArc::new(
        Point::new(32.0, 28.0),
        12.0,
        0.0,
        std::f32::consts::FRAC_PI_2,
        4.0,
        SdfLineCap::Round,
    );
    scene.push_arc_shadow(
        arc,
        RectShadowOptions::new(4.0, 4.0, 4.0, 0.5),
        Color::BLACK,
    );
    scene.push_sdf_arc(arc, red);

    let mut renderer = Renderer::new(72, 64, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(40, 36), [220, 64, 72, 255]);
    let near_shadow = renderer.image().rgba8_at(48, 40);
    assert!(
        near_shadow[0] < 245,
        "expected arc shadow near offset arc, got {near_shadow:?}"
    );
    assert_eq!(renderer.image().rgba8_at(64, 12), [255, 255, 255, 255]);
}

#[test]
fn nested_clip_layers_intersect_child_fill() {
    let mut scene = Scene::new(96, 96);
    scene.push_clip_layer(
        Rect::new(16.0, 16.0, 80.0, 80.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_clip_layer(
        Rect::new(40.0, 8.0, 88.0, 88.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_path(
        Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
    assert_eq!(renderer.image().rgba8_at(24, 48), [255, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(84, 48), [255, 255, 255, 255]);
}

#[test]
fn opacity_layer_isolates_offscreen_children() {
    let mut scene = Scene::new(16, 16);
    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 128, 0));
    scene.push_filter_layer(Filter::Opacity(1.0), Region::rect(full, Radius::ZERO));
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 128, 128]);
}

#[test]
fn outer_clip_does_not_clip_filter_source_before_blur() {
    let mut scene = Scene::new(96, 96);
    let clip = Circle::new((48.0, 48.0), 24.0).to_path(0.1);
    scene.push_clip_layer(clip.clone(), Affine::IDENTITY, FillRule::NonZero, 0.1);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 8.0,
            std_dev_y: 8.0,
        },
        Region::Path {
            path: clip,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
        },
    );
    scene.push_path(
        Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(70, 48), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(74, 48), [255, 255, 255, 255]);
}

#[test]
fn filter_blur_outputs_expanded_bounds() {
    let mut scene = Scene::new(96, 96);
    let sample_rect = Rect::new(32.0, 32.0, 64.0, 64.0);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
        },
        Region::rect(sample_rect, Radius::ZERO),
    );
    scene.push_path(
        sample_rect.to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    let expanded_px = renderer.image().rgba8_at(28, 48);
    assert_eq!(expanded_px[0], 255);
    assert!(
        expanded_px[1] < 245 && expanded_px[2] < 245,
        "expected blur outside sample region, got {expanded_px:?}"
    );
}

#[test]
fn filter_offset_preserves_source_outside_canvas() {
    let mut scene = Scene::new(48, 16);
    let source = Rect::new(-16.0, 0.0, 0.0, 16.0);
    scene.push_filter_layer(
        Filter::Offset { dx: 16.0, dy: 0.0 },
        Region::rect(source, Radius::ZERO),
    );
    scene.push_rect(source, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.pop_layer();

    let mut renderer = Renderer::new(48, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(0, 8), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(15, 8), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 8), [0, 0, 0, 0]);
}

#[test]
fn filter_offset_with_huge_source_keeps_only_visible_dependency_window() {
    let mut scene = Scene::new(64, 16);
    let source = Rect::new(-100_000.0, 0.0, 100_000.0, 16.0);
    scene.push_filter_layer(
        Filter::Offset { dx: 20.0, dy: 0.0 },
        Region::rect(source, Radius::ZERO),
    );
    scene.push_rect(source, crate::Radius::ZERO, Color::from_rgb8(0, 128, 0));
    scene.pop_layer();

    let mut renderer = Renderer::new(64, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(0, 8), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(63, 8), [0, 128, 0, 255]);
}

#[test]
fn backdrop_filter_samples_existing_target() {
    let mut scene = Scene::new(48, 24);
    scene.push_rect(
        Rect::new(0.0, 0.0, 48.0, 24.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(8.0, 4.0, 32.0, 20.0), Radius::ZERO),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(48, 24, Color::WHITE);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(12, 8), [0, 255, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(4, 8), [255, 0, 0, 255]);
}

#[test]
fn backdrop_rect_liquid_glass_refracts_rect_edge_without_moving_center() {
    let mut scene = Scene::new(64, 32);
    for x in 0..64 {
        let v = (x * 4) as u8;
        scene.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 32.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, v, v),
        );
    }

    scene.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::TRANSPARENT,
            refraction_thickness: 8.0,
            refraction_dispersion: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(16.0, 4.0, 48.0, 28.0), Radius::all(6.0)),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(64, 32, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 16), [32, 32, 32, 255]);
    assert_rgb_close(renderer.image().rgba8_at(32, 16), [128, 128, 128, 255], 1);
    let edge = renderer.image().rgba8_at(17, 16);
    assert!(
        edge[0] > 68,
        "expected left glass edge to sample farther into the gradient, got {edge:?}"
    );
}

#[test]
fn backdrop_rect_liquid_glass_does_not_shadow_outside_sample_region() {
    let mut scene = Scene::new(64, 40);
    scene.push_rect(
        Rect::new(0.0, 0.0, 64.0, 40.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    scene.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::TRANSPARENT,
            refraction_dispersion: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(16.0, 8.0, 48.0, 24.0), Radius::all(4.0)),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(64, 40, Color::TRANSPARENT);
    renderer.render(&scene);

    let outside_region = renderer.image().rgba8_at(32, 30);
    assert_eq!(outside_region, [255, 255, 255, 255]);
}

#[test]
fn opacity_layer_composites_children_as_isolated_group() {
    let mut scene = Scene::new(96, 96);
    scene.push_opacity_layer(
        Rect::new(8.0, 8.0, 88.0, 88.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_path(
        Rect::new(16.0, 16.0, 64.0, 64.0).to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_path(
        Rect::new(32.0, 32.0, 80.0, 80.0).to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_rgb_close(renderer.image().rgba8_at(40, 40), [255, 128, 128, 255], 1);
}

#[test]
fn blend_layer_composites_tile_group_through_layer_mask() {
    let mut scene = Scene::new(96, 96);
    scene.push_path(
        Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
        Color::from_rgb8(128, 128, 128),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_blend_layer(
        Rect::new(16.0, 16.0, 80.0, 80.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_path(
        Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(96, 96, Color::WHITE);
    renderer.render(&scene);

    assert_rgb_close(renderer.image().rgba8_at(40, 40), [128, 0, 0, 255], 1);
    assert_eq!(renderer.image().rgba8_at(8, 40), [128, 128, 128, 255]);
}

#[test]
fn blend_layer_isolates_offscreen_children() {
    let mut scene = Scene::new(16, 16);
    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.push_filter_layer(Filter::Opacity(1.0), Region::rect(full, Radius::ZERO));
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 255, 0));
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_rgb_close(renderer.image().rgba8_at(4, 8), [0, 128, 0, 255], 1);
    assert_eq!(renderer.image().rgba8_at(12, 8), [128, 128, 128, 255]);
}

#[test]
fn isolate_layer_gives_child_blend_a_transparent_group_backdrop() {
    let mut scene = Scene::new(16, 16);
    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    scene.push_isolate_layer(full.to_path(0.0), Affine::IDENTITY, 0.0);
    scene.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(8, 8), [255, 0, 0, 255]);
}

#[test]
fn mask_layer_applies_alpha_coverage_and_region() {
    let mut mask_scene = Scene::new(16, 16);
    mask_scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgba8(255, 255, 255, 128),
    );

    let mut scene = Scene::new(16, 16);
    scene.push_mask_layer(
        mask_scene,
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(4, 8), [128, 0, 0, 128]);
    assert_eq!(renderer.image().rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn mask_layer_uses_luminance_by_default_semantics() {
    let mut mask_scene = Scene::new(16, 16);
    mask_scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );

    let mut scene = Scene::new(16, 16);
    scene.push_mask_layer(
        mask_scene,
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
            kind: MaskKind::Luminance,
        },
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    let px = renderer.image().rgba8_at(8, 8);
    assert!(
        px[1].abs_diff(54) <= 1 && px[3].abs_diff(54) <= 1,
        "got {px:?}"
    );
}
