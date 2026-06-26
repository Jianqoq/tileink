use std::ops::Range;

use peniko::BlendMode;

use crate::shared::{bounds::Bounds, layer::Layer};

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;
pub(crate) type ClipStackEntry = u32;

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

/// GPU-friendly linear execution plan for one command list.
///
/// `clip_stack_data` stores ordered clip stack snapshots referenced by
/// `ExecOp::DrawBatch`. Opacity and blend are expressed as explicit begin/end
/// ops so their group lifetime survives across multiple draw batches.
#[derive(Debug)]
pub(crate) struct ExecPlan {
    pub ops: Vec<ExecOp>,
    /// Batch-local clip stack snapshots in user nesting order.
    pub clip_stack_data: Vec<ClipStackEntry>,
}

#[derive(Debug)]
pub(crate) enum ExecOp {
    DrawBatch {
        draws: Range<usize>,
        /// Active clip stack for this batch in nesting order. This is
        /// stack state, not coverage.
        clip_stack: Range<usize>,
    },
    BeginClip {
        draw: usize,
        bounds: Bounds,
    },
    EndClip,
    BeginOpacity {
        draw: usize,
        opacity: f32,
        bounds: Bounds,
    },
    EndOpacity {
        draw: usize,
        opacity: f32,
        bounds: Bounds,
    },
    BeginBlend {
        draw: usize,
        mode: BlendMode,
        bounds: Bounds,
    },
    EndBlend {
        draw: usize,
        mode: BlendMode,
        bounds: Bounds,
    },
    OffscreenLayer {
        layer: Layer,
        children: Vec<ExecOp>,
    },
}
