use peniko::kurbo::{Affine, BezPath};

use crate::shared::{bounds::Bounds, brush::Brush};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Clone)]
pub struct Fill {
    pub(crate) path: BezPath,
    pub(crate) brush: Brush,
    pub(crate) transform: Affine,
    pub(crate) rule: FillRule,
    pub(crate) tolerance: f64,
    pub(crate) bounds: Bounds,
}
