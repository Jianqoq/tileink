use super::*;
use crate::{
    NodeGeneration, RetainedNodeId,
    canvas::{RetainedFrameDelta, RetainedNodeKind, RetainedNodePatch, RetainedNodeState},
};

fn frame(nodes: &[(u64, u64, Bounds)]) -> RetainedFrame {
    let nodes = nodes
        .iter()
        .enumerate()
        .map(|(order, (id, revision, bounds))| RetainedNodeState {
            id: RetainedNodeId::for_owner(*id),
            revision: NodeGeneration::new(*revision),
            bounds: *bounds,
            order: order as u32,
            kind: RetainedNodeKind::Scene,
            placement_bits: None,
        })
        .collect::<Vec<_>>();
    let node_index = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    RetainedFrame {
        root: RetainedNodeId::for_owner(1),
        logical_size: (128, 64),
        physical_size: (128, 64),
        scale_bits: 1.0f32.to_bits(),
        nodes: nodes.into(),
        node_index: std::rc::Rc::new(node_index),
        state_pages: std::rc::Rc::new(Default::default()),
        invalidated_bounds: Vec::new(),
        invalidate_all: false,
        incremental_complete: true,
        version: None,
        delta: None,
        damage_history: crate::canvas::damage_history::DamageHistory::default(),
        dependency_free: false,
        requires_damage_propagation: true,
    }
}

mod frame_diff;
mod planning;
