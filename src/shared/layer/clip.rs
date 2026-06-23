use peniko::kurbo::{Affine, BezPath};

use crate::shared::bounds::Bounds;

#[derive(Clone, Debug)]
pub struct Clip {
    pub(crate) path: BezPath,
    pub(crate) bounds: Bounds,
    pub(crate) transform: Affine,
    pub(crate) tolerance: f64,
}
