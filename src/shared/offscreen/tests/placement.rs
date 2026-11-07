//! Local surfaces change presentation coordinates, never an already transformed draw's payload.
use super::*;
use crate::{Radius, ShadowOptions, TextContext, TextFontSystem, TextLayoutOptions};

fn text_layout() -> crate::TextLayout {
    let mut fonts = TextFontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    TextContext::new().layout(&mut fonts, TextLayoutOptions::new("Ink", 20.))
}

#[test]
fn rebasing_transformed_offscreen_ink_preserves_its_physical_position() {
    let text = text_layout();
    assert!(
        !text.is_empty(),
        "the checked-in font must shape the fixture"
    );
    for dpi in [1., 1.25, 1.5, 2.] {
        let mut source = Canvas::new(100, 100, dpi);
        let brush = Brush::from_gradient_with_ramp_size(
            &peniko::Gradient::new_linear((4., 6.), (80., 70.))
                .with_stops([Color::BLACK, Color::WHITE]),
            4,
        );
        source.push_rect(Rect::new(4., 6., 80., 70.), Radius::all(4.), brush.clone());
        source.push_path(
            Rect::new(14., 16., 40., 30.).to_path(0.1),
            brush.clone(),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );
        source
            .push_rect_shadow(
                Rect::new(4., 6., 80., 70.),
                Radius::all(4.),
                ShadowOptions::new(2., 6., 4., 0.5),
                brush.clone(),
            )
            .unwrap();
        source
            .push_text_layout(&text, (20., 40.).into(), brush)
            .unwrap();
        for (transform, origin) in [
            (Affine::translate((60., 72.)) * Affine::scale(0.99), (5, 15)),
            (
                Affine::translate((380., 72.)) * Affine::scale(0.99),
                (325, 15),
            ),
            (
                Affine::translate((150., 72.)) * Affine::rotate(0.23),
                (80, 20),
            ),
            (
                Affine::translate((150., 72.)) * Affine::skew(0.15, -0.1),
                (80, 20),
            ),
        ] {
            let mut canvas = Canvas::new(780, 360, dpi);
            canvas.append_transformed(&source, transform);
            let (local, map) = translated_scene_for_draws(
                &canvas,
                LocalSpace::new(Bounds::new(
                    origin.0,
                    origin.1,
                    origin.0 + 330,
                    origin.1 + 300,
                )),
                &[0, 1, 2, 3],
            );
            for (&old, &new) in &map {
                let original = canvas.draw_records[old];
                let moved = local.draw_records[new];
                let world = original.transform.transform_point(20., 25.);
                let expected = (world.0 - origin.0 as f32, world.1 - origin.1 as f32);
                assert_point(moved.transform.transform_point(20., 25.), expected);
                // Fine rendering must still sample the same brush, SDF, shadow, and glyph
                // coordinates after the target origin changes (root-cause regression).
                assert_point(
                    moved
                        .inverse_transform
                        .transform_point(expected.0, expected.1),
                    (20., 25.),
                );
                assert_eq!(moved.local_pixel_bounds, original.local_pixel_bounds);
                assert_eq!(
                    &canvas.brush_blob[original.brush_offset as usize
                        ..(original.brush_offset + original.brush_len) as usize],
                    &local.brush_blob[moved.brush_offset as usize
                        ..(moved.brush_offset + moved.brush_len) as usize],
                );
                if let Some(range) = original.sdf_range() {
                    assert_eq!(
                        canvas.sdf_blob[range],
                        local.sdf_blob[moved.sdf_range().unwrap()]
                    );
                }
                if let Some(range) = original.sdf_shadow_range() {
                    assert_eq!(
                        canvas.sdf_shadow_blob[range],
                        local.sdf_shadow_blob[moved.sdf_shadow_range().unwrap()]
                    );
                }
            }
            assert_eq!(canvas.text_glyphs.len(), local.text_glyphs.len());
            for (old, new) in canvas.text_glyphs.iter().zip(&local.text_glyphs) {
                assert_eq!((old.cache_key, old.x, old.y), (new.cache_key, new.x, new.y));
            }
            let original = canvas.draw_records[0];
            let moved = local.draw_records[map[&0]];
            let Sdf::Rect(original_rect) =
                decode_sdf(&canvas.sdf_blob, original.sdf_offset, original.sdf_len).unwrap()
            else {
                panic!("rectangle")
            };
            let Sdf::Rect(local_rect) =
                decode_sdf(&local.sdf_blob, moved.sdf_offset, moved.sdf_len).unwrap()
            else {
                panic!("rectangle")
            };
            let center = |rect: crate::shared::sdf::rect::Rect| {
                (
                    (rect.start.x + rect.end.x) as f32 / 2.,
                    (rect.start.y + rect.end.y) as f32 / 2.,
                )
            };
            let p = center(original_rect);
            let expected = original.transform.transform_point(p.0, p.1);
            let local_center = center(local_rect);
            let actual = moved
                .transform
                .transform_point(local_center.0, local_center.1);
            assert!(
                (actual.0 - (expected.0 - origin.0 as f32)).abs() < 0.001
                    && (actual.1 - (expected.1 - origin.1 as f32)).abs() < 0.001,
                "offscreen origin was applied in scaled local coordinates: actual {actual:?}, expected {expected:?} minus {origin:?}"
            );
            assert_eq!(moved.local_pixel_bounds, original.local_pixel_bounds);
            assert_eq!(original_rect.start, local_rect.start);
            assert_eq!(original_rect.end, local_rect.end);
            let source_path = canvas.path_records[0];
            let local_path = local.path_records[0];
            assert_eq!(local_path.transform, local.draw_records[map[&1]].transform);
            assert_eq!(
                canvas.lines[source_path.line_start as usize].p0,
                local.lines[local_path.line_start as usize].p0
            );
            let p = canvas.lines[source_path.line_start as usize].p0;
            let expected = source_path.transform.transform_point(p[0], p[1]);
            let actual = local_path.transform.transform_point(p[0], p[1]);
            assert!(
                (actual.0 - (expected.0 - origin.0 as f32)).abs() < 0.001
                    && (actual.1 - (expected.1 - origin.1 as f32)).abs() < 0.001
            );
            let tile_bbox = local.draw_records[map[&1]]
                .tile_bbox(local.width_in_tiles(), local.height_in_tiles());
            let required: u32 = local
                .lines
                .iter()
                .map(|line| {
                    let mut line = *line;
                    let p0 = local_path.transform.transform_point(line.p0[0], line.p0[1]);
                    let p1 = local_path.transform.transform_point(line.p1[0], line.p1[1]);
                    line.p0 = [p0.0, p0.1];
                    line.p1 = [p1.0, p1.1];
                    crate::shared::scan_line::line_scanned_tile_count(line, tile_bbox)
                })
                .sum();
            assert!(
                local_path.segment_capacity >= required,
                "offscreen scan capacity must cover transformed lines"
            );
        }
    }
}

fn assert_point(actual: (f32, f32), expected: (f32, f32)) {
    assert!(
        (actual.0 - expected.0).abs() < 0.001 && (actual.1 - expected.1).abs() < 0.001,
        "coordinate frame mismatch: actual {actual:?}, expected {expected:?}"
    );
}
