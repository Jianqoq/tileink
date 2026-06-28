use std::ops::Range;

use peniko::BlendMode;

use crate::shared::layer::{Layer, mask::Mask};

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LayerStackEntry {
    Clip { draw: u32 },
    Opacity { draw: u32, opacity: f32 },
    Blend { draw: u32, mode: BlendMode },
}

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
    MaskLayer {
        layer: Mask,
        content: CommandListId,
        mask: CommandListId,
    },
}

/// GPU-friendly linear execution plan for one command list.
///
/// `layer_stack_data` stores ordered fused layer stack snapshots referenced by
/// `ExecOp::DrawBatch`. The coarse stage replays this stack into tile-local
/// begin/end particles so fine owns clip, opacity, and blend semantics.
#[derive(Clone, Debug)]
pub(crate) struct ExecPlan {
    pub ops: Vec<ExecOp>,
    /// Batch-local fused layer stack snapshots in user nesting order.
    pub layer_stack_data: Vec<LayerStackEntry>,
}

#[derive(Clone, Debug)]
pub(crate) enum ExecOp {
    DrawBatch {
        draws: Range<usize>,
        /// Active fused layer stack for this batch in nesting order. This is
        /// stack state, not coverage.
        layer_stack: Range<usize>,
    },
    BeginClip,
    EndClip,
    BeginOpacity,
    EndOpacity,
    BeginBlend,
    EndBlend,
    OffscreenLayer {
        draw: usize,
        layer: Layer,
        outer_stack: Range<usize>,
        children: Vec<ExecOp>,
    },
    OffscreenMaskLayer {
        layer: Mask,
        outer_stack: Range<usize>,
        content: Vec<ExecOp>,
        mask: Vec<ExecOp>,
    },
}
