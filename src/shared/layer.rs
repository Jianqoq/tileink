pub(crate) mod backdrop;
pub(crate) mod blend;
pub(crate) mod clip;
pub(crate) mod filter;
pub(crate) mod mask;

use std::sync::Arc;

use usvg::{Transform, tiny_skia_path::IntRect};

use crate::shared::{
    bounds::Bounds,
    layer::{backdrop::BackdropRegion, blend::Blend, clip::Clip, filter::Filter, mask::MaskMode},
    sdf::Sdf,
};

pub enum Layer {
    Clip(Clip),
    ClipSdf {
        sdf: Sdf,
        bounds: Bounds,
    },
    Opacity {
        opacity: f32,
    },
    Blend {
        blend: Blend,
    },
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
            Layer::Opacity { .. } => LayerKind::Opacity,
            Layer::Blend { .. } => LayerKind::Blend,
            Layer::Filter { .. } => LayerKind::Filter,
            Layer::SvgFilter { .. } => LayerKind::SvgFilter,
            Layer::BackdropFilter { .. } => LayerKind::BackdropFilter,
            Layer::Mask { .. } => LayerKind::Mask,
        }
    }
}
