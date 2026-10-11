use super::*;
use crate::SdfLineCap;

#[test]
#[should_panic(expected = "encoded canvas brush")]
fn append_rejects_a_corrupt_present_brush_instead_of_removing_it() {
    let mut child = test_scene();
    let brush = Brush::from_image_key_natural(
        ImageKey::new(7),
        [0.0; 2],
        (2, 2),
        Extend::Pad,
        PatternSampling::Nearest,
        255,
    )
    .unwrap();
    child.push_rect(Rect::new(4.0, 4.0, 20.0, 20.0), Radius::ZERO, brush);
    child.brush_blob[8] = u32::MAX;
    test_scene().append(&child, (2.0, 3.0));
}

#[test]
#[should_panic(expected = "encoded canvas SDF")]
fn append_rejects_a_corrupt_present_sdf_instead_of_removing_it() {
    let mut child = test_scene();
    child.push_line(
        SdfLine::new(
            (4.0, 4.0).into(),
            (20.0, 20.0).into(),
            2.0,
            SdfLineCap::Round,
        ),
        Color::WHITE,
    );
    child.sdf_blob[6] = f32::NAN.to_bits();
    test_scene().append(&child, (2.0, 3.0));
}

#[test]
#[should_panic(expected = "encoded canvas SDF shadow")]
fn append_rejects_a_corrupt_present_shadow_instead_of_removing_it() {
    let mut child = test_scene();
    child.push_line_shadow(
        SdfLine::new(
            (4.0, 4.0).into(),
            (20.0, 20.0).into(),
            2.0,
            SdfLineCap::Round,
        ),
        ShadowOptions::new(2.0, 3.0, 4.0, 0.5),
        Color::WHITE,
    );
    child.sdf_shadow_blob[6] = f32::NAN.to_bits();
    test_scene().append(&child, (2.0, 3.0));
}
