use std::{collections::HashSet, rc::Rc};

use rustc_hash::FxHashMap as HashMap;

use crate::shared::bounds::Bounds;

/// Stable identity for retained content.
///
/// `owner` normally identifies a UI element and `slot` distinguishes multiple
/// independently changing scenes owned by that element. IDs must be unique
/// inside one retained root.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RetainedNodeId {
    pub owner: u64,
    pub slot: u32,
}

impl RetainedNodeId {
    pub const DEFAULT_SLOT: u32 = 0;

    pub const fn new(owner: u64, slot: u32) -> Self {
        Self { owner, slot }
    }

    pub const fn for_owner(owner: u64) -> Self {
        Self::new(owner, Self::DEFAULT_SLOT)
    }
}

/// Internal generation attached to persistent materialized commands and surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct NodeGeneration(u64);

impl NodeGeneration {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for NodeGeneration {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

/// Stable internal identity for a persistent layer boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PersistentLayerKey {
    pub(crate) id: RetainedNodeId,
    pub(crate) revision: NodeGeneration,
}

impl PersistentLayerKey {
    pub const fn new(id: RetainedNodeId, revision: NodeGeneration) -> Self {
        Self { id, revision }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RetainedSurfaceId {
    pub(crate) node: RetainedNodeId,
    pub(crate) slot: u32,
}

impl RetainedSurfaceId {
    pub(crate) const fn new(node: RetainedNodeId, slot: u32) -> Self {
        Self { node, slot }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetainedNodeKind {
    Scene,
    Layer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RetainedNodeState {
    pub(crate) id: RetainedNodeId,
    pub(crate) revision: NodeGeneration,
    pub(crate) bounds: Bounds,
    pub(crate) order: u32,
    pub(crate) kind: RetainedNodeKind,
    pub(crate) placement_bits: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetainedNodePatch {
    pub(crate) old: Option<RetainedNodeState>,
    pub(crate) new: Option<RetainedNodeState>,
    /// Explicit output damage for bounded transforms. Raw geometry bounds remain exact, while
    /// the retained output domain and its conservative spatial index stay fixed.
    pub(crate) damage: Option<Bounds>,
}

impl RetainedNodePatch {
    pub(crate) fn damage_regions(self) -> impl Iterator<Item = Bounds> {
        let old = self.old.map(|state| state.bounds);
        let new = self
            .new
            .map(|state| state.bounds)
            .filter(|bounds| Some(*bounds) != old);
        let explicit = self
            .damage
            .filter(|bounds| Some(*bounds) != old && Some(*bounds) != new);
        [old, new, explicit].into_iter().flatten()
    }

    pub(crate) fn damage_bounds(self) -> Option<Bounds> {
        self.damage_regions().reduce(Bounds::union)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFrameDelta {
    pub(crate) from_version: u64,
    pub(crate) to_version: u64,
    pub(crate) patches: Rc<[RetainedNodePatch]>,
    pub(crate) previous: Option<Rc<RetainedFrameDelta>>,
    pub(crate) depth: u16,
    pub(crate) damage: Rc<[(RetainedNodeId, Bounds)]>,
    /// Backdrops affected by this persistent journal delta. When complete, the renderer can skip
    /// the generic command-tree propagation pass because `damage` already contains their output.
    pub(crate) dirty_backdrops: Rc<[RetainedNodeId]>,
    pub(crate) backdrop_damage_complete: bool,
    pub(crate) index: Rc<HashMap<RetainedNodeId, usize>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFrame {
    pub(crate) root: RetainedNodeId,
    pub(crate) logical_size: (u32, u32),
    pub(crate) physical_size: (u32, u32),
    pub(crate) scale_bits: u32,
    pub(crate) nodes: Rc<[RetainedNodeState]>,
    pub(crate) node_index: Rc<HashMap<RetainedNodeId, usize>>,
    /// Copy-on-write state pages compact long content-only delta chains without cloning every
    /// retained node. Topology overlays remain in `delta` until a hierarchy rebuild.
    pub(crate) state_pages: Rc<HashMap<usize, Rc<[RetainedNodeState]>>>,
    pub(crate) invalidated_bounds: Vec<Bounds>,
    pub(crate) invalidate_all: bool,
    pub(crate) incremental_complete: bool,
    /// Persistent-scene version and immutable journal overlay.
    pub(crate) version: Option<u64>,
    pub(crate) delta: Option<Rc<RetainedFrameDelta>>,
    /// No layer/filter/mask can propagate leaf damage outside the changed node bounds.
    pub(crate) dependency_free: bool,
    /// Backdrops require walking command ancestry after retained diffing to discover changed
    /// sampled background. Filter descendants already carry filter-expanded frame bounds; manual
    /// invalidation is handled separately because it has no retained node attribution.
    pub(crate) requires_damage_propagation: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RetainedDamage {
    pub(crate) node_bounds: HashMap<RetainedNodeId, Bounds>,
    pub(crate) unattributed: Vec<Bounds>,
}

pub(crate) struct RetainedDamagePropagation {
    pub(crate) bounds: Vec<Bounds>,
    pub(crate) dirty_backdrops: HashSet<RetainedNodeId>,
}
