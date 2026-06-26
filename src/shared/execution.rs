use std::ops::Range;

use crate::shared::layer::{Layer, blend::Blend};

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

pub(crate) struct ExecPlan {
    pub nodes: Vec<ExecNode>,
    pub clip_layers: Vec<Layer>,
    pub clip_stack_data: Vec<u32>,
    pub opacity_stack_data: Vec<f32>,
    pub blend_layers: Vec<Blend>,
    pub blend_stack_data: Vec<u32>,
}

pub(crate) enum ExecNode {
    DrawBatch {
        draws: Range<usize>,
        state: BatchState,
        clip_stack: Range<usize>,
        opacity_stack: Range<usize>,
        blend_stack: Range<usize>,
    },
    OffscreenLayer {
        layer: Layer,
        children: Vec<ExecNode>,
    },
}
