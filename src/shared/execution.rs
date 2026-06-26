use std::ops::Range;

use peniko::BlendMode;

use crate::shared::layer::Layer;

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;

#[derive(Default)]
pub(crate) struct CommandList {
    pub commands: Vec<Command>,
}

pub(crate) enum Command {
    Draw(usize),
    Layer {
        draw: usize,
        layer: Layer,
        children: CommandListId,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BatchState {
    pub clip_depth: u8,
    pub blend_depth: u8,
    pub opacity_depth: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FusedLayerEntry {
    Clip { draw_ix: u32 },
    Opacity { draw_ix: u32, opacity: f32 },
    Blend { draw_ix: u32, mode: BlendMode },
}

impl FusedLayerEntry {
    pub fn draw_ix(self) -> u32 {
        match self {
            Self::Clip { draw_ix }
            | Self::Opacity { draw_ix, .. }
            | Self::Blend { draw_ix, .. } => draw_ix,
        }
    }
}

/// Flattened execution plan for one command list.
///
/// `fused_layers` stores ordered stack snapshots referenced by
/// `ExecNode::DrawBatch`. These are not coverage bounds. They only describe
/// which fused layers are active for a batch and in which nesting order.
///
/// Semantically, clip / opacity / blend should also carry their own raster
/// ranges or bounds so execution can restrict work to the layer's coverage
/// instead of implicitly treating the layer state as full-screen.
pub(crate) struct ExecPlan {
    pub nodes: Vec<ExecNode>,
    /// Batch-local fused layer snapshots in user nesting order.
    pub fused_layers: Vec<FusedLayerEntry>,
}

pub(crate) enum ExecNode {
    DrawBatch {
        draws: Range<usize>,
        state: BatchState,
        /// Active fused layer stack for this batch in nesting order. This is
        /// stack state, not coverage.
        fused_layers: Range<usize>,
    },
    OffscreenLayer {
        layer: Layer,
        children: Vec<ExecNode>,
    },
}
