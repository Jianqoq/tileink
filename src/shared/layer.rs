pub(crate) mod blend;
pub(crate) mod clip;
pub(crate) mod filter;
pub(crate) mod mask;
pub(crate) mod opacity;
pub(crate) mod region;

use std::sync::Arc;

use usvg::{Transform, tiny_skia_path::IntRect};

use crate::shared::{
    bounds::Bounds,
    layer::{
        blend::Blend, clip::Clip, filter::Filter, mask::MaskMode, opacity::Opacity, region::Region,
    },
    sdf::Sdf,
};

#[derive(Clone, Debug)]
pub enum Layer {
    Clip(Clip),
    ClipSdf { sdf: Sdf, bounds: Bounds },
    Opacity(Opacity),
    Blend(Blend),
    Filter { filter: Filter, region: Region },
    Backdrop { filter: Filter, region: Region },
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

impl Layer {
    pub(crate) fn kind(&self) -> LayerKind {
        match self {
            Layer::Clip(_) => LayerKind::Clip,
            Layer::ClipSdf { .. } => LayerKind::ClipSdf,
            Layer::Opacity(_) => LayerKind::Opacity,
            Layer::Blend(_) => LayerKind::Blend,
            Layer::Filter { .. } => LayerKind::Filter,
            Layer::Backdrop { .. } => LayerKind::Backdrop,
        }
    }
}
