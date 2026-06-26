pub(crate) mod backdrop;
pub(crate) mod blend;
pub(crate) mod clip;
pub(crate) mod filter;
pub(crate) mod mask;
pub(crate) mod opacity;

use std::sync::Arc;

use usvg::{Transform, tiny_skia_path::IntRect};

use crate::shared::{
    bounds::Bounds,
    layer::{
        backdrop::BackdropRegion, blend::Blend, clip::Clip, filter::Filter, mask::MaskMode,
        opacity::Opacity,
    },
    sdf::Sdf,
};

#[derive(Clone, Debug)]
pub enum Layer {
    Clip(Clip),
    ClipSdf {
        sdf: Sdf,
        bounds: Bounds,
    },
    Opacity(Opacity),
    Blend(Blend),
    Filter {
        filter: Filter,
    },
    SvgFilter {
        filters: Vec<Arc<usvg::filter::Filter>>,
        transform: Transform,
        max_bounds: IntRect,
    },
    BackdropFilter {
        filter: Filter,
        region: BackdropRegion,
    },
    Mask {
        mode: MaskMode,
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
    SvgFilter,
    BackdropFilter,
    Mask,
}

impl Layer {
    pub(crate) fn kind(&self) -> LayerKind {
        match self {
            Layer::Clip(_) => LayerKind::Clip,
            Layer::ClipSdf { .. } => LayerKind::ClipSdf,
            Layer::Opacity(_) => LayerKind::Opacity,
            Layer::Blend(_) => LayerKind::Blend,
            Layer::Filter { .. } => LayerKind::Filter,
            Layer::SvgFilter { .. } => LayerKind::SvgFilter,
            Layer::BackdropFilter { .. } => LayerKind::BackdropFilter,
            Layer::Mask { .. } => LayerKind::Mask,
        }
    }
}
