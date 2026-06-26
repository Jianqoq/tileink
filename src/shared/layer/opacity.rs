use peniko::kurbo::{Affine, BezPath};

use crate::shared::bounds::Bounds;

#[derive(Clone, Debug)]
pub struct Opacity {
    pub(crate) path: BezPath,
    pub(crate) bounds: Bounds,
    pub(crate) transform: Affine,
    pub(crate) tolerance: f64,
    pub(crate) opacity: f32,
}

impl Opacity {
    pub(crate) fn new(
        path: BezPath,
        bounds: Bounds,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) -> Self {
        Self {
            path,
            bounds,
            transform,
            tolerance,
            opacity,
        }
    }
}
