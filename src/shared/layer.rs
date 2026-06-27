pub(crate) mod blend;
pub mod filter;
pub(crate) mod opacity;
pub mod region;

use crate::shared::{
    bounds::Bounds,
    layer::{blend::Blend, filter::Filter, opacity::Opacity, region::Region},
    sdf::Sdf,
};

#[derive(Clone, Debug)]
pub enum Layer {
    Clip,
    ClipSdf {
        sdf: Sdf,
        bounds: Bounds,
    },
    Opacity(Opacity),
    Blend(Blend),
    Filter {
        filter: Filter,
        sample_region: Region,
    },
    Backdrop {
        filter: Filter,
        sample_region: Region,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerKind {
    Root,
    Clip,
    ClipSdf,
    Opacity,
    Blend,
    Filter,
    Backdrop,
}
