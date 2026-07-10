use std::{ops::Range, sync::Arc};

use peniko::BlendMode;

use crate::{
    Canvas, RetainedNodeId, SceneRevision,
    canvas::RetainedSurfaceId,
    shared::layer::{Layer, mask::Mask},
};

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LayerStackEntry {
    Clip { draw: u32 },
    Opacity { draw: u32, opacity: f32 },
    Blend { draw: u32, mode: BlendMode },
}

#[derive(Clone, Default)]
pub(crate) struct CommandList {
    pub commands: Vec<Command>,
}

#[derive(Clone)]
pub(crate) enum Command {
    Draw(usize),
    RetainedScene {
        id: RetainedNodeId,
        revision: SceneRevision,
        canvas: Arc<Canvas>,
        offset: (f64, f64),
    },
    RetainedNode {
        id: RetainedNodeId,
        revision: SceneRevision,
        children: CommandListId,
    },
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

impl ExecPlan {
    /// Removes artificial GPU batch boundaries introduced only by retained command scopes.
    ///
    /// Retained nodes carry CPU-side identity and offscreen ownership, but adjacent plain draws
    /// with the same effective layer stack have exactly the same raster semantics as one batch.
    /// Coalescing them is important for component trees: otherwise every widget scope would emit
    /// another coarse/fine dispatch pair even during a full redraw.
    pub(crate) fn coalesce_draw_batches(&mut self) {
        coalesce_draw_batches_in(&mut self.ops, &self.layer_stack_data);
    }
}

fn coalesce_draw_batches_in(ops: &mut Vec<ExecOp>, layer_stacks: &[LayerStackEntry]) {
    for op in ops.iter_mut() {
        match op {
            ExecOp::OffscreenLayer { children, .. } => {
                coalesce_draw_batches_in(children, layer_stacks);
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                coalesce_draw_batches_in(content, layer_stacks);
                coalesce_draw_batches_in(mask, layer_stacks);
            }
            _ => {}
        }
    }

    let mut coalesced = Vec::with_capacity(ops.len());
    for op in ops.drain(..) {
        if let ExecOp::DrawBatch { draws, layer_stack } = &op
            && let Some(ExecOp::DrawBatch {
                draws: previous_draws,
                layer_stack: previous_stack,
            }) = coalesced.last_mut()
            && previous_draws.end == draws.start
            && layer_stacks[previous_stack.clone()] == layer_stacks[layer_stack.clone()]
        {
            previous_draws.end = draws.end;
            continue;
        }
        coalesced.push(op);
    }
    *ops = coalesced;
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
        retained_id: Option<RetainedSurfaceId>,
        draw: usize,
        layer: Layer,
        outer_stack: Range<usize>,
        children: Vec<ExecOp>,
    },
    OffscreenMaskLayer {
        retained_id: Option<RetainedSurfaceId>,
        layer: Mask,
        outer_stack: Range<usize>,
        content: Vec<ExecOp>,
        mask: Vec<ExecOp>,
    },
}
