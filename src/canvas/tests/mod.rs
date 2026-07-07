use super::*;
use std::ops::Range;

use peniko::kurbo::PathEl;

fn test_scene() -> Canvas {
    Canvas::new(64, 64, 1.0)
}
fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    BezPath::from_vec(vec![
        PathEl::MoveTo((x0, y0).into()),
        PathEl::LineTo((x1, y0).into()),
        PathEl::LineTo((x1, y1).into()),
        PathEl::LineTo((x0, y1).into()),
        PathEl::ClosePath,
    ])
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}
fn assert_layer_stack(plan: &ExecPlan, layer_stack: Range<usize>, expected: &[LayerStackEntry]) {
    let actual = &plan.layer_stack_data[layer_stack];
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual, expected);
}

mod compile;
mod filters;
mod paths;
mod primitives;
