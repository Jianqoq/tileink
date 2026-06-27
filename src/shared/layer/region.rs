use peniko::kurbo::{Affine, BezPath, Rect};

use crate::shared::sdf::rect::Radius;

/// Geometry that defines where a backdrop filter is visible.
///
/// This region is independent of content drawn inside the backdrop layer.
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
