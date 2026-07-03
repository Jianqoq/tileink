use super::*;

#[test]
#[should_panic(expected = "RectLiquidGlass is a rounded-rectangle backdrop effect")]
fn push_filter_layer_rejects_rect_liquid_glass() {
    let mut canvas = test_scene();
    canvas.push_filter_layer(
        Filter::RectLiquidGlass(crate::RectLiquidGlass::default()),
        Region::rect(Rect::new(0.0, 0.0, 20.0, 20.0), Radius::all(4.0)),
    );
}

#[test]
#[should_panic(expected = "RectLiquidGlass requires Region::Rect")]
fn push_backdrop_layer_rejects_rect_liquid_glass_path_region() {
    let mut canvas = test_scene();
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(crate::RectLiquidGlass::default()),
        Region::path(rect_path(0.0, 0.0, 20.0, 20.0), Affine::IDENTITY, 0.1),
    );
}
