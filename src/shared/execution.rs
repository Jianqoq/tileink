//! Compiled draw and offscreen operations. Fused groups use batch stack snapshots only.
mod batches;
mod fragments;
mod layer_stacks;
mod layers;
mod retained;

pub(crate) use layers::OffscreenLayer;

use batches::max_batch_id;
use layer_stacks::{compact_layer_stacks, offset_layer_stack_ranges};
use retained::{find_retained_offscreen, replace_retained_offscreen};

use std::{collections::HashMap, ops::Range, rc::Rc};

use peniko::BlendMode;

use crate::{
    NodeRevision, PersistentLayerKey, RetainedNodeId,
    canvas::RetainedSurfaceId,
    shared::layer::{Layer, mask::Mask},
};

pub(crate) type CommandListId = usize;
pub(crate) const ROOT_COMMAND_LIST_ID: CommandListId = 0;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RetainedBatchOwner {
    pub(crate) node: RetainedNodeId,
    pub(crate) branch: RetainedBatchBranch,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RetainedBatchBranch {
    Content,
    Mask,
}

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
    MaterializedRetainedScene {
        id: RetainedNodeId,
        revision: NodeRevision,
        /// The scene owns every list copied from its source, including internal layer lists.
        fragment: Range<CommandListId>,
    },
    Layer {
        retained: Option<PersistentLayerKey>,
        layer: Layer,
        children: CommandListId,
    },
    MaskLayer {
        retained: Option<PersistentLayerKey>,
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
    /// Physical draw slots in painter order. Retained arenas may keep stable slots that are not
    /// contiguous, so tile binning must not infer order from the draw-record buffer.
    pub draw_order: Rc<Vec<u32>>,
    /// Stable physical draw slot to coarse batch membership.
    pub draw_batch_ids: Rc<Vec<u32>>,
    /// Stable batch assigned to each retained layer branch, including empty branches.
    pub retained_batch_ids: HashMap<RetainedBatchOwner, u32>,
    /// Fused layer hidden draw to the stack entries that reference it. A descriptor update can
    /// patch its own entries without scanning every unrelated retained layer.
    pub(crate) layer_stack_locations: HashMap<u32, Vec<usize>>,
    /// Direct root batches are indexable when the plan contains only fused layers. Offscreen
    /// plans stay on the recursive executor because their control flow carries surface effects.
    pub(crate) direct_root_batch_ops: Option<HashMap<u32, usize>>,
}

#[derive(Clone, Debug)]
pub(crate) enum ExecOp {
    DrawBatch {
        draws: Rc<Vec<usize>>,
        batch_id: u32,
        owners: Rc<Vec<RetainedBatchOwner>>,
        /// Active fused layer stack for this batch in nesting order. This is
        /// stack state, not coverage.
        layer_stack: Range<usize>,
    },
    OffscreenLayer {
        retained_id: Option<RetainedSurfaceId>,
        layer: OffscreenLayer,
        /// Ancestor clips only: opacity/blend ancestors with offscreen children
        /// own separate surfaces, so their groups never enter this range.
        outer_stack: Range<usize>,
        children: Vec<ExecOp>,
    },
    OffscreenMaskLayer {
        retained_id: Option<RetainedSurfaceId>,
        layer: Mask,
        /// Ancestor clip range, with the same contract as OffscreenLayer.
        outer_stack: Range<usize>,
        content: Vec<ExecOp>,
        mask: Vec<ExecOp>,
    },
}
