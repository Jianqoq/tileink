use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{Canvas, Filter, ProgressiveBlur, Radius, Region};

#[test]
fn progressive_blur_records_as_a_filter_and_backdrop() {
    let blur = ProgressiveBlur::new(Point::new(0.0, 8.0), Point::new(0.0, 48.0), 12.0);
    let mut canvas = Canvas::new(64, 64, 2.0);
    let region = Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO);
    canvas.push_filter_layer(Filter::ProgressiveBlur(blur), region.clone());
    canvas.push_rect(Rect::new(8.0, 8.0, 56.0, 56.0), Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    canvas.push_backdrop_layer(Filter::ProgressiveBlur(blur), region);
    canvas.pop_layer();
    assert!(canvas.is_closed_for_append());
}
