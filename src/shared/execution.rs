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

/// Flattened execution plan for one command list.
///
/// `clip_stack_data`, `opacity_stack_data`, and `blend_stack_data` are stack
/// snapshots referenced by `ExecNode::DrawBatch`. These are not coverage
/// bounds. They only describe which fused layers are active for a batch.
///
/// Semantically, clip / opacity / blend should also carry their own raster
/// ranges or bounds so execution can restrict work to the layer's coverage
/// instead of implicitly treating the layer state as full-screen.
pub(crate) struct ExecPlan {
    pub nodes: Vec<ExecNode>,
    /// Unique clip-layer payloads shared by batch-local clip stack slices.
    pub clip_layers: Vec<Layer>,
    /// Batch-local clip stack snapshots. Entries index into `clip_layers`.
    pub clip_stack_data: Vec<u32>,
    /// Batch-local opacity stack snapshots.
    pub opacity_stack_data: Vec<f32>,
    /// Unique blend modes shared by batch-local blend stack slices.
    pub blend_layers: Vec<BlendMode>,
    /// Batch-local blend stack snapshots. Entries index into `blend_layers`.
    pub blend_stack_data: Vec<u32>,
}

pub(crate) enum ExecNode {
    DrawBatch {
        draws: Range<usize>,
        state: BatchState,
        /// Active clip stack for this batch. This is stack state, not clip
        /// coverage. A later plan revision should pair clip state with its own
        /// tile/pixel range.
        clip_stack: Range<usize>,
        /// Active opacity stack for this batch. This does not mean opacity
        /// applies to the whole target; opacity should eventually carry its own
        /// coverage range as well.
        opacity_stack: Range<usize>,
        /// Active blend stack for this batch. Like opacity/clip, this is only
        /// state and should eventually be paired with layer-local coverage.
        blend_stack: Range<usize>,
    },
    OffscreenLayer {
        layer: Layer,
        children: Vec<ExecNode>,
    },
}
