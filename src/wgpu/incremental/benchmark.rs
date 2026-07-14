//! Criterion adapter for retained-frame diffing without renderer or GPU timing noise.

use super::*;
use crate::{
    NodeGeneration, RetainedNodeId,
    canvas::{RetainedNodeKind, RetainedNodeState},
};

#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum FrameDiffBenchmarkCase {
    Static,
    OneRevision,
    ManyRevisions,
    ReorderDisjoint,
    ReorderOverlapping,
    InsertRemove,
}

#[doc(hidden)]
pub struct FrameDiffBenchmark {
    previous: RetainedFrame,
    current: RetainedFrame,
    damage: DamageTiles,
    retained: RetainedDamage,
    scratch: FrameDiffScratch,
}

impl FrameDiffBenchmark {
    pub fn new(count: usize, case: FrameDiffBenchmarkCase) -> Self {
        let size = (2048, 2048);
        let mut previous = nodes(count, size, false);
        let mut current = previous.clone();
        match case {
            FrameDiffBenchmarkCase::Static => {}
            FrameDiffBenchmarkCase::OneRevision => {
                current[count / 2].revision = NodeGeneration::new(1)
            }
            FrameDiffBenchmarkCase::ManyRevisions => {
                for node in &mut current[count / 2..count / 2 + 1_024.min(count / 2)] {
                    node.revision = NodeGeneration::new(1);
                }
            }
            FrameDiffBenchmarkCase::ReorderDisjoint => {
                let start = count / 2;
                current[start..start + 1_024.min(count / 2)].reverse();
            }
            FrameDiffBenchmarkCase::ReorderOverlapping => {
                previous = nodes(count, size, true);
                current = previous.clone();
                current.reverse();
            }
            FrameDiffBenchmarkCase::InsertRemove => {
                let changed = 256.min(count / 2);
                current.truncate(count - changed);
                current.extend(nodes(changed, size, false).into_iter().map(|mut node| {
                    node.id = RetainedNodeId::for_owner(count as u64 + 10_000 + node.order as u64);
                    node
                }));
            }
        }
        for (order, node) in current.iter_mut().enumerate() {
            node.order = order as u32;
        }
        Self {
            previous: frame(previous, size),
            current: frame(current, size),
            damage: DamageTiles::new(size),
            retained: RetainedDamage::default(),
            scratch: FrameDiffScratch::default(),
        }
    }

    pub fn diff(&mut self) -> (u32, usize) {
        diff_frames_reusing(
            &self.previous,
            &self.current,
            &mut self.damage,
            &mut self.retained,
            &mut self.scratch,
        );
        let result = (self.damage.len(), self.retained.node_bounds.len());
        self.damage.clear_for_benchmark();
        self.retained.node_bounds.clear();
        self.retained.unattributed.clear();
        result
    }
}

fn nodes(count: usize, size: (u32, u32), overlapping: bool) -> Vec<RetainedNodeState> {
    let tiles_width = size.0 / TILE_SIZE;
    (0..count)
        .map(|index| {
            let tile = index as u32 % (tiles_width * (size.1 / TILE_SIZE));
            let x = if overlapping {
                32
            } else {
                tile % tiles_width * TILE_SIZE
            };
            let y = if overlapping {
                32
            } else {
                tile / tiles_width * TILE_SIZE
            };
            RetainedNodeState {
                id: RetainedNodeId::for_owner(index as u64 + 2),
                revision: NodeGeneration::new(0),
                bounds: Bounds::new(
                    x as i32,
                    y as i32,
                    (x + TILE_SIZE) as i32,
                    (y + TILE_SIZE) as i32,
                ),
                order: index as u32,
                kind: RetainedNodeKind::Scene,
                placement_bits: None,
            }
        })
        .collect()
}

fn frame(nodes: Vec<RetainedNodeState>, size: (u32, u32)) -> RetainedFrame {
    let node_index = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    RetainedFrame {
        root: RetainedNodeId::for_owner(1),
        logical_size: size,
        physical_size: size,
        scale_bits: 1.0f32.to_bits(),
        nodes: nodes.into(),
        node_index: Rc::new(node_index),
        state_pages: Rc::new(Default::default()),
        invalidated_bounds: Vec::new(),
        invalidate_all: false,
        incremental_complete: true,
        version: None,
        delta: None,
        dependency_free: false,
        requires_damage_propagation: true,
    }
}
