//! Recording descriptors reference encoded geometry only for geometric layers.
//! SDF geometry and bounds belong to the scene buffers, not the execution topology.
pub mod filter;
pub mod mask;
pub mod region;

use crate::shared::layer::{filter::Filter, region::Region};
use peniko::BlendMode;

#[derive(Clone, Debug)]
pub(crate) enum Layer {
    Clip {
        draw: usize,
    },
    ClipSdf {
        draw: usize,
    },
    Isolate {
        draw: usize,
    },
    Opacity {
        draw: usize,
        opacity: f32,
    },
    Blend {
        draw: usize,
        mode: BlendMode,
    },
    Filter {
        filter: Filter,
        sample_region: Region,
    },
    Backdrop {
        filter: Filter,
        sample_region: Region,
        opacity: Option<f32>,
    },
}

impl Layer {
    /// Effects have no geometry arena slot or fused-stack dependency.
    pub(crate) const fn hidden_draw(&self) -> Option<usize> {
        match self {
            Self::Clip { draw }
            | Self::ClipSdf { draw }
            | Self::Isolate { draw }
            | Self::Opacity { draw, .. }
            | Self::Blend { draw, .. } => Some(*draw),
            Self::Filter { .. } | Self::Backdrop { .. } => None,
        }
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "Layer owns filters whose destructors are not const"
    )]
    pub(crate) fn remap_draw(mut self, offset: usize) -> Self {
        match &mut self {
            Self::Clip { draw }
            | Self::ClipSdf { draw }
            | Self::Isolate { draw }
            | Self::Opacity { draw, .. }
            | Self::Blend { draw, .. } => *draw += offset,
            Self::Filter { .. } | Self::Backdrop { .. } => {}
        }
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerKind {
    Clip,
    ClipSdf,
    Isolate,
    Opacity,
    Blend,
    Mask,
    Filter,
    Backdrop,
}
