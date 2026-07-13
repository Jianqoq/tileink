use super::prelude::*;

const ORDER_STEP: u128 = 1 << 64;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SceneVersion(pub(crate) u64);

impl SceneVersion {
    pub const INITIAL: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum RetainedChildBranch {
    #[default]
    Content,
    Mask,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RetainedParent {
    pub node: RetainedNodeId,
    pub branch: RetainedChildBranch,
}

impl RetainedParent {
    pub const fn content(node: RetainedNodeId) -> Self {
        Self {
            node,
            branch: RetainedChildBranch::Content,
        }
    }

    pub const fn mask(node: RetainedNodeId) -> Self {
        Self {
            node,
            branch: RetainedChildBranch::Mask,
        }
    }
}

#[derive(Clone, Debug)]
pub enum RetainedLayerDescriptor {
    ClipPath {
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    },
    ClipSdf {
        sdf: Sdf,
        transform: Affine,
    },
    Isolate {
        path: BezPath,
        transform: Affine,
        tolerance: f64,
    },
    Opacity {
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    },
    Blend {
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    },
    Filter {
        filter: Filter,
        sample_region: Region,
    },
    Backdrop {
        filter: Filter,
        sample_region: Region,
    },
    Mask(Mask),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetainedSceneError {
    DuplicateNode(RetainedNodeId),
    MissingNode(RetainedNodeId),
    InvalidParentBranch(RetainedNodeId),
    InvalidSibling(RetainedNodeId),
    Cycle(RetainedNodeId),
    CannotRemoveRoot,
    ScaleMismatch,
    UnclosedCanvas,
    InvalidPosition,
    InvalidTransform,
    InvalidSize,
}

impl fmt::Display for RetainedSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode(id) => write!(f, "retained node {id:?} already exists"),
            Self::MissingNode(id) => write!(f, "retained node {id:?} does not exist"),
            Self::InvalidParentBranch(id) => {
                write!(f, "retained node {id:?} does not support that child branch")
            }
            Self::InvalidSibling(id) => write!(f, "retained sibling {id:?} is not in that parent"),
            Self::Cycle(id) => write!(f, "reparenting retained node {id:?} would create a cycle"),
            Self::CannotRemoveRoot => write!(f, "the retained root cannot be removed"),
            Self::ScaleMismatch => {
                write!(f, "retained child canvas scale does not match the scene")
            }
            Self::UnclosedCanvas => write!(f, "retained child canvas has unclosed layers"),
            Self::InvalidPosition => write!(f, "retained geometry contains non-finite coordinates"),
            Self::InvalidTransform => {
                write!(f, "retained node transform must be finite and invertible")
            }
            Self::InvalidSize => write!(
                f,
                "retained scene size and scale must be positive and finite"
            ),
        }
    }
}

impl Error for RetainedSceneError {}

#[derive(Clone)]
pub(crate) enum NodeKind {
    Group,
    Scene {
        canvas: Rc<Canvas>,
        transform: Affine,
        /// Fixed logical output region for translation-only animation.
        ///
        /// The caller must clip the scene to this region. Retained damage can then stay fixed
        /// while the scene moves, avoiding old/new per-draw damage expansion. Ordinary
        /// `set_transform` clears this hint so non-translation edits retain exact semantics.
        translation_damage: Option<Rect>,
    },
    Layer(RetainedLayerDescriptor),
}

#[derive(Clone, Default)]
pub(crate) struct ChildList {
    pub(crate) order: BTreeMap<u128, RetainedNodeId>,
    pub(crate) keys: HashMap<RetainedNodeId, u128>,
}

pub(crate) struct ChildInsertion {
    pub(crate) key: u128,
    pub(crate) before_rebalance: Option<ChildList>,
}

impl ChildList {
    pub(crate) fn key_of(&self, id: RetainedNodeId) -> Option<u128> {
        self.keys.get(&id).copied()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &RetainedNodeId> {
        self.order.values()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub(crate) fn insert_at(&mut self, key: u128, id: RetainedNodeId) {
        self.order.insert(key, id);
        self.keys.insert(id, key);
    }

    pub(crate) fn insert_before(
        &mut self,
        id: RetainedNodeId,
        before: Option<RetainedNodeId>,
    ) -> Result<ChildInsertion, RetainedSceneError> {
        if self.order.is_empty() {
            self.insert_at(ORDER_STEP, id);
            return Ok(ChildInsertion {
                key: ORDER_STEP,
                before_rebalance: None,
            });
        }
        let (lower, upper) = if let Some(before) = before {
            let upper = self
                .key_of(before)
                .ok_or(RetainedSceneError::InvalidSibling(before))?;
            let lower = self.order.range(..upper).next_back().map(|(key, _)| *key);
            (lower, Some(upper))
        } else {
            (self.order.last_key_value().map(|(key, _)| *key), None)
        };
        let key = match (lower, upper) {
            (Some(lower), Some(upper)) if upper - lower > 1 => lower + (upper - lower) / 2,
            (None, Some(upper)) if upper > 1 => upper / 2,
            (Some(lower), None) if u128::MAX - lower >= ORDER_STEP => lower + ORDER_STEP,
            _ => {
                let before_rebalance = self.clone();
                self.rebalance();
                let mut inserted = self.insert_before(id, before)?;
                inserted.before_rebalance = Some(before_rebalance);
                return Ok(inserted);
            }
        };
        self.insert_at(key, id);
        Ok(ChildInsertion {
            key,
            before_rebalance: None,
        })
    }

    pub(crate) fn remove(&mut self, id: RetainedNodeId) -> Option<u128> {
        let key = self.keys.remove(&id)?;
        self.order.remove(&key)?;
        Some(key)
    }

    pub(crate) fn rebalance(&mut self) {
        let children = self.order.values().copied().collect::<Vec<_>>();
        self.order.clear();
        self.keys.clear();
        let step = u128::MAX / (children.len() as u128 + 1);
        for (index, child) in children.into_iter().enumerate() {
            self.insert_at(step * (index as u128 + 1), child);
        }
    }
}

#[derive(Clone)]
pub(crate) struct SceneNode {
    pub(crate) kind: NodeKind,
    pub(crate) parent: Option<RetainedParent>,
    pub(crate) content: ChildList,
    pub(crate) mask: ChildList,
    pub(crate) instance: u64,
    pub(crate) generation: u64,
}

impl SceneNode {
    pub(crate) fn group(parent: Option<RetainedParent>, instance: u64) -> Self {
        Self {
            kind: NodeKind::Group,
            parent,
            content: ChildList::default(),
            mask: ChildList::default(),
            instance,
            generation: 0,
        }
    }

    pub(crate) fn children(
        &self,
        branch: RetainedChildBranch,
    ) -> Result<&ChildList, RetainedSceneError> {
        match branch {
            RetainedChildBranch::Content => Ok(&self.content),
            RetainedChildBranch::Mask
                if matches!(self.kind, NodeKind::Layer(RetainedLayerDescriptor::Mask(_))) =>
            {
                Ok(&self.mask)
            }
            RetainedChildBranch::Mask => Err(RetainedSceneError::InvalidParentBranch(
                self.parent
                    .map_or(RetainedNodeId::for_owner(0), |parent| parent.node),
            )),
        }
    }

    pub(crate) fn children_mut(
        &mut self,
        id: RetainedNodeId,
        branch: RetainedChildBranch,
    ) -> Result<&mut ChildList, RetainedSceneError> {
        match branch {
            RetainedChildBranch::Content => Ok(&mut self.content),
            RetainedChildBranch::Mask
                if matches!(self.kind, NodeKind::Layer(RetainedLayerDescriptor::Mask(_))) =>
            {
                Ok(&mut self.mask)
            }
            RetainedChildBranch::Mask => Err(RetainedSceneError::InvalidParentBranch(id)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SceneChangeSet {
    pub(crate) changed_nodes: HashSet<RetainedNodeId>,
    pub(crate) changed_layers: HashSet<RetainedNodeId>,
    pub(crate) removed_nodes: HashSet<RetainedNodeId>,
    pub(crate) invalidated_rects: Vec<Rect>,
    pub(crate) invalidate_all: bool,
    pub(crate) topology_changed: bool,
    pub(crate) hierarchy_changed: bool,
    pub(crate) surface_changed: bool,
}

impl SceneChangeSet {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.changed_nodes
            .extend(other.changed_nodes.iter().copied());
        self.changed_layers
            .extend(other.changed_layers.iter().copied());
        self.removed_nodes
            .extend(other.removed_nodes.iter().copied());
        self.invalidated_rects
            .extend_from_slice(&other.invalidated_rects);
        self.invalidate_all |= other.invalidate_all;
        self.topology_changed |= other.topology_changed;
        self.hierarchy_changed |= other.hierarchy_changed;
        self.surface_changed |= other.surface_changed;
    }
}
