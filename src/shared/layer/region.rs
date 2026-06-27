use peniko::kurbo::{Affine, BezPath, Rect};

use crate::shared::sdf::rect::Radius;

/// Geometry used to derive the source area sampled by a layer filter.
///
/// Filters may expand this area internally; for example blur needs pixels
/// outside the original sample region so the filtered output can spread.
#[derive(Clone, Debug)]
pub enum Region {
    Rect {
        rect: Rect,
        radius: Radius,
    },
    Path {
        path: BezPath,
        transform: Affine,
        tolerance: f64,
    },
}

impl Region {
    pub fn rect(rect: Rect, radius: Radius) -> Self {
        Self::Rect { rect, radius }
    }

    pub fn path(path: BezPath, transform: Affine, tolerance: f64) -> Self {
        Self::Path {
            path,
            transform,
            tolerance,
        }
    }
}
