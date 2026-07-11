use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use peniko::{
    Compose, Mix,
    kurbo::{Affine, BezPath, PathEl, Point, Rect},
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::{
    Canvas, FillRule, Filter, Mask, Region, RetainedLayerKey, RetainedNodeId, SceneRevision, Sdf,
    canvas::{
        PainterKey, RetainedFrameDelta, RetainedNodeKind, RetainedNodePatch, RetainedSceneCache,
        SceneBufferChanges,
    },
    shared::{
        bounds::Bounds,
        draw_record::{DrawRecord, DrawTagWord, FillRuleWord},
        execution::{Command, CommandList, RetainedBatchBranch},
        image::Image,
        image_resource::{ImageKey, ImageResourceStore},
        layer::{Layer, filter},
        line::Line,
        path::PathRecord,
        scene_arena::{ArenaAllocation, SceneArena},
    },
    text::{CanvasGlyph, TextRun},
};

const JOURNAL_CAPACITY: usize = 256;
const ORDER_STEP: u128 = 1 << 64;
static NEXT_SCENE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SceneVersion(u64);

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
    ClipSdf(Sdf),
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
            Self::InvalidPosition => write!(f, "retained node position must be finite"),
            Self::InvalidSize => write!(
                f,
                "retained scene size and scale must be positive and finite"
            ),
        }
    }
}

impl Error for RetainedSceneError {}

#[derive(Clone)]
enum NodeKind {
    Group,
    Scene {
        canvas: Arc<Canvas>,
        position: Point,
    },
    Layer(RetainedLayerDescriptor),
}

#[derive(Clone, Default)]
struct ChildList {
    order: BTreeMap<u128, RetainedNodeId>,
    keys: HashMap<RetainedNodeId, u128>,
}

impl ChildList {
    fn key_of(&self, id: RetainedNodeId) -> Option<u128> {
        self.keys.get(&id).copied()
    }

    fn values(&self) -> impl Iterator<Item = &RetainedNodeId> {
        self.order.values()
    }

    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    fn insert_at(&mut self, key: u128, id: RetainedNodeId) {
        self.order.insert(key, id);
        self.keys.insert(id, key);
    }

    fn insert_before(
        &mut self,
        id: RetainedNodeId,
        before: Option<RetainedNodeId>,
    ) -> Result<u128, RetainedSceneError> {
        if self.order.is_empty() {
            self.insert_at(ORDER_STEP, id);
            return Ok(ORDER_STEP);
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
                self.rebalance();
                return self.insert_before(id, before);
            }
        };
        self.insert_at(key, id);
        Ok(key)
    }

    fn remove(&mut self, id: RetainedNodeId) -> Option<u128> {
        let key = self.keys.remove(&id)?;
        self.order.remove(&key)?;
        Some(key)
    }

    fn rebalance(&mut self) {
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
struct SceneNode {
    kind: NodeKind,
    parent: Option<RetainedParent>,
    content: ChildList,
    mask: ChildList,
    generation: u64,
}

impl SceneNode {
    fn group(parent: Option<RetainedParent>) -> Self {
        Self {
            kind: NodeKind::Group,
            parent,
            content: ChildList::default(),
            mask: ChildList::default(),
            generation: 0,
        }
    }

    fn children(&self, branch: RetainedChildBranch) -> Result<&ChildList, RetainedSceneError> {
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

    fn children_mut(
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
    changed_layers: HashSet<RetainedNodeId>,
    pub(crate) removed_nodes: HashSet<RetainedNodeId>,
    pub(crate) invalidated_rects: Vec<Rect>,
    pub(crate) invalidate_all: bool,
    pub(crate) topology_changed: bool,
    hierarchy_changed: bool,
    pub(crate) surface_changed: bool,
}

impl SceneChangeSet {
    fn merge(&mut self, other: &Self) {
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

struct SceneChunk {
    generation: u64,
    source_canvas: Option<Arc<Canvas>>,
    position_bits: Option<(u64, u64)>,
    // Chunks have a single owner. Keeping their mutable encoding behind an Arc made every
    // revision pay an atomic uniqueness check and made newly inserted chunks allocate twice.
    canvas: Canvas,
    lines: ArenaAllocation,
    paths: ArenaAllocation,
    draws: ArenaAllocation,
    brushes: ArenaAllocation,
    sdfs: ArenaAllocation,
    shadows: ArenaAllocation,
    glyphs: Option<ArenaAllocation>,
    runs: Option<ArenaAllocation>,
    backdrops: ArenaAllocation,
    segments: ArenaAllocation,
    plan_fingerprint: u64,
    plain_fragment: bool,
    // Persistent damage propagation cannot rediscover ordinary backdrop commands by walking the
    // materialized command tree every frame. Cache their translated dependency geometry with the
    // chunk so an earlier node mutation only visits actual backdrop owners.
    backdrop_dependencies: Vec<BackdropDependency>,
}

#[derive(Clone, Copy)]
struct BackdropDependency {
    dependency: Bounds,
    output: Bounds,
    output_outset: i32,
}

#[derive(Clone, Copy)]
struct NodeRebuild {
    plan_dirty: bool,
    position_only: bool,
}

#[derive(Clone, Copy)]
struct SceneChunkLengths {
    lines: usize,
    paths: usize,
    draws: usize,
    brushes: usize,
    sdfs: usize,
    shadows: usize,
    runs: usize,
    backdrops: usize,
    segments: usize,
}

impl SceneChunkLengths {
    fn from_canvas(canvas: &Canvas) -> Self {
        Self {
            lines: canvas.lines.len(),
            paths: canvas.path_records.len(),
            draws: canvas.draw_records.len(),
            brushes: canvas.brush_blob.len(),
            sdfs: canvas.sdf_blob.len(),
            shadows: canvas.sdf_shadow_blob.len(),
            runs: canvas.text_runs.len(),
            backdrops: canvas.backdrop_pool_capacity as usize,
            segments: canvas.tile_cnt as usize,
        }
    }
}

#[derive(Clone)]
struct SceneCommandLocation {
    parent_list: usize,
    command_index: usize,
    fragment_start: usize,
    fragment_count: usize,
}

#[derive(Clone, Copy)]
struct LayerCommandLocation {
    parent_list: usize,
    command_index: usize,
}

struct RootPlanFragment {
    ops: std::ops::Range<usize>,
    command_lists: std::ops::Range<usize>,
    nodes: HashSet<RetainedNodeId>,
    reassigned_batches: HashMap<RetainedNodeId, u32>,
    removed_batch: Option<(usize, crate::shared::execution::ExecOp)>,
}

struct MaterializedArenas {
    lines: SceneArena<Line>,
    paths: SceneArena<PathRecord>,
    draws: SceneArena<DrawRecord>,
    brushes: SceneArena<u32>,
    sdfs: SceneArena<u32>,
    shadows: SceneArena<u32>,
    glyphs: Option<SceneArena<CanvasGlyph>>,
    runs: SceneArena<TextRun>,
    backdrops: SceneArena<u8>,
    segments: SceneArena<u8>,
}

impl Default for MaterializedArenas {
    fn default() -> Self {
        Self {
            lines: SceneArena::new(Line::default()),
            paths: SceneArena::new(PathRecord::default()),
            draws: SceneArena::new(inactive_draw()),
            brushes: SceneArena::new(0),
            sdfs: SceneArena::new(0),
            shadows: SceneArena::new(0),
            glyphs: None,
            runs: SceneArena::new(TextRun {
                glyph_start: 0,
                glyph_count: 0,
            }),
            backdrops: SceneArena::new(0),
            segments: SceneArena::new(0),
        }
    }
}

/// Persistent CPU materialization for the stateful API.
///
/// Scene data lives in stable arenas and only changed nodes are translated or copied. Command
/// metadata is rebuilt when physical allocations or topology change; the later persistent-plan
/// layer consumes the same stable draw slots without changing this storage contract.
pub(crate) struct PersistentSceneMaterializer {
    scene_id: u64,
    version: SceneVersion,
    canvas: Arc<Canvas>,
    chunks: HashMap<RetainedNodeId, SceneChunk>,
    arenas: MaterializedArenas,
    nested_cache: RetainedSceneCache,
    plan_cache_key: u64,
    scene_command_locations: HashMap<RetainedNodeId, SceneCommandLocation>,
    layer_command_locations: HashMap<RetainedNodeId, LayerCommandLocation>,
    resource_refs: HashMap<ImageKey, (Arc<Image>, usize)>,
    dependency_free: bool,
    layer_nodes: HashSet<RetainedNodeId>,
    nonlocal_dependencies: HashSet<RetainedNodeId>,
    painter_bases: HashMap<RetainedNodeId, Arc<[u128]>>,
    painter_parents: HashMap<RetainedNodeId, RetainedParent>,
    flat_plan_has_draws: bool,
    node_bounds: HashMap<RetainedNodeId, Bounds>,
    raw_node_bounds: HashMap<RetainedNodeId, Bounds>,
    node_tiles: Vec<HashSet<RetainedNodeId>>,
    node_batches: HashMap<RetainedNodeId, u32>,
    container_batches: HashMap<RetainedParent, u32>,
    root_plan_fragments: HashMap<RetainedNodeId, RootPlanFragment>,
}

static NEXT_PLAN_CACHE_KEY: AtomicU64 = AtomicU64::new(1);

fn next_plan_cache_key() -> u64 {
    (1 << 63) | NEXT_PLAN_CACHE_KEY.fetch_add(1, Ordering::Relaxed)
}

impl PersistentSceneMaterializer {
    pub(crate) fn new(scene: &RetainedScene) -> Self {
        let mut materializer = Self {
            scene_id: scene.id,
            version: SceneVersion::INITIAL,
            canvas: Arc::new(Canvas::new_retained(
                scene.width,
                scene.height,
                scene.scale,
                scene.root,
            )),
            chunks: HashMap::default(),
            arenas: MaterializedArenas::default(),
            nested_cache: RetainedSceneCache::default(),
            plan_cache_key: next_plan_cache_key(),
            scene_command_locations: HashMap::default(),
            layer_command_locations: HashMap::default(),
            resource_refs: HashMap::default(),
            dependency_free: false,
            layer_nodes: HashSet::default(),
            nonlocal_dependencies: HashSet::default(),
            painter_bases: HashMap::default(),
            painter_parents: HashMap::default(),
            flat_plan_has_draws: false,
            node_bounds: HashMap::default(),
            raw_node_bounds: HashMap::default(),
            node_tiles: Vec::new(),
            node_batches: HashMap::default(),
            container_batches: HashMap::default(),
            root_plan_fragments: HashMap::default(),
        };
        materializer.rebuild_all(scene);
        materializer
    }

    pub(crate) fn scene_id(&self) -> u64 {
        self.scene_id
    }

    pub(crate) fn version(&self) -> SceneVersion {
        self.version
    }

    pub(crate) fn canvas(&self) -> Arc<Canvas> {
        self.canvas.clone()
    }

    /// Consumes the scene journal and returns whether prepared CPU/GPU scene data became stale.
    /// Raster-only invalidation advances the version without forcing materialization or upload.
    pub(crate) fn update(&mut self, scene: &RetainedScene) -> bool {
        if self.scene_id != scene.id {
            *self = Self::new(scene);
            return true;
        }
        if self.version == scene.version {
            return false;
        }
        let Some(changes) = scene.changes_since(self.version) else {
            self.rebuild_all(scene);
            return true;
        };
        if changes.surface_changed {
            self.rebuild_all(scene);
            return true;
        }
        let analysis_profile = crate::wgpu::start_cpu_scope("retained.materialize.analysis");
        let previous_dependency_free = self.dependency_free;
        for id in &changes.removed_nodes {
            self.layer_nodes.remove(id);
            self.nonlocal_dependencies.remove(id);
        }
        if changes.topology_changed {
            for &id in &changes.changed_nodes {
                let node = scene.nodes.get(&id);
                if node.is_some_and(|node| matches!(&node.kind, NodeKind::Layer(_))) {
                    self.layer_nodes.insert(id);
                } else {
                    self.layer_nodes.remove(&id);
                }
                // Scene chunks may contain ordinary backdrop commands. Keep their existing
                // membership until rebuild_node refreshes it from the newly encoded chunk.
                if !node.is_some_and(|node| matches!(&node.kind, NodeKind::Scene { .. })) {
                    self.nonlocal_dependencies.remove(&id);
                }
            }
        }
        let expanded_topology_changes =
            if changes.hierarchy_changed {
                let mut topology_changes = changes.clone();
                for &id in &changes.changed_nodes {
                    if scene.nodes.get(&id).is_some_and(|node| {
                        matches!(node.kind, NodeKind::Group | NodeKind::Layer(_))
                    }) {
                        let mut leaves = Vec::new();
                        collect_scene_leaves(scene, id, &mut leaves);
                        topology_changes.changed_nodes.extend(leaves);
                    }
                }
                Some(topology_changes)
            } else {
                None
            };
        let topology_changes = expanded_topology_changes.as_ref().unwrap_or(&changes);

        let scene_data_changed = changes.topology_changed
            || !changes.changed_nodes.is_empty()
            || !changes.removed_nodes.is_empty();
        let previous_frame = Arc::make_mut(&mut self.canvas)
            .retained_frame_override
            .clone();
        let mut delta_eligible =
            !changes.topology_changed && !changes.surface_changed && self.dependency_free;
        let topology_delta_eligible = changes.topology_changed
            && self.dependency_free
            && previous_frame.as_ref().is_some_and(|frame| {
                changes.changed_nodes.iter().all(|id| {
                    matches!(
                        scene.nodes.get(id).map(|node| &node.kind),
                        Some(NodeKind::Group)
                    ) || frame.node_state(*id).is_none()
                })
            });
        let root_reorder_candidate = changes.topology_changed
            && changes.removed_nodes.is_empty()
            && previous_frame.as_ref().is_some_and(|frame| {
                changes.changed_nodes.iter().all(|id| {
                    frame.node_state(*id).is_some()
                        && scene.nodes.get(id).is_some_and(|node| {
                            matches!(node.kind, NodeKind::Scene { .. })
                                && node.parent == self.painter_parents.get(id).copied()
                        })
                })
            });
        let old_painter_bases = if root_reorder_candidate {
            topology_changes
                .changed_nodes
                .iter()
                .filter_map(|id| self.painter_bases.get(id).map(|base| (*id, base.clone())))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::default()
        };
        let layer_update_candidate = changes.topology_changed
            && !changes.hierarchy_changed
            && changes.removed_nodes.is_empty()
            && changes.changed_nodes == changes.changed_layers;
        let root_layer_remove_candidate = (!changes.removed_nodes.is_empty())
            .then(|| {
                self.root_plan_fragments.iter().find_map(|(&id, fragment)| {
                    let location = self.layer_command_locations.get(&id)?;
                    (fragment.nodes == changes.removed_nodes && location.parent_list == 0)
                        .then_some(id)
                })
            })
            .flatten();
        let stable_batch_candidate = !changes.topology_changed
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| {
                    matches!(node.kind, NodeKind::Group)
                        || (self.node_batches.contains_key(id)
                            && self
                                .chunks
                                .get(id)
                                .is_some_and(|chunk| chunk.plain_fragment))
                })
            });
        let mut reorder_damage = Vec::new();
        let mut plain_topology_damage = Vec::new();
        let root_painter_update = changes.topology_changed
            && self.dependency_free
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| match node.kind {
                    NodeKind::Group => node.content.is_empty(),
                    NodeKind::Scene { .. } => true,
                    NodeKind::Layer(_) => false,
                })
            });

        let compactions_before = self.arena_compactions();
        let flat_plan_had_draws = self.flat_plan_has_draws;
        let removed_plain_leaves = changes.removed_nodes.iter().all(|id| {
            self.chunks
                .get(id)
                .is_some_and(|chunk| chunk.plain_fragment && self.node_batches.contains_key(id))
        });
        let mut commands_dirty = changes.topology_changed && !layer_update_candidate;
        let mut plan_dirty = changes.topology_changed;
        let mut plan_compiled_during_update = false;
        let mut layer_plan_patches = Vec::new();
        let mut plan_layer_stack_changes = Vec::new();
        let mut layer_bounds_stable = true;
        let mut chunks_rebuilt = 0;
        let mut position_plan_patches = Vec::new();
        let mut unpatchable_plan_change = changes.topology_changed;
        drop(analysis_profile);
        let chunk_profile = crate::wgpu::start_cpu_scope("retained.materialize.chunks");
        for id in &changes.removed_nodes {
            if let Some(chunk) = self.chunks.remove(id) {
                self.remove_chunk(chunk);
            }
        }

        for id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(id) else {
                continue;
            };
            if matches!(node.kind, NodeKind::Group)
                || self
                    .chunks
                    .get(id)
                    .is_some_and(|chunk| chunk.generation == node.generation)
            {
                continue;
            }
            let old_layer_bounds = layer_update_candidate
                .then(|| self.chunks.get(id).map(chunk_layer_influence_bounds))
                .flatten();
            let rebuilt = self.rebuild_node(scene, *id);
            plan_dirty |= rebuilt.plan_dirty;
            if rebuilt.plan_dirty && rebuilt.position_only {
                position_plan_patches.push(*id);
            } else {
                unpatchable_plan_change |= rebuilt.plan_dirty;
            }
            chunks_rebuilt += 1;
            if !changes.topology_changed && matches!(node.kind, NodeKind::Scene { .. }) {
                self.patch_scene_commands(scene, *id);
            } else if layer_update_candidate && matches!(node.kind, NodeKind::Layer(_)) {
                let (old, new) = self.patch_layer_command(scene, *id);
                layer_plan_patches.push((*id, old, new));
                layer_bounds_stable &=
                    old_layer_bounds == self.chunks.get(id).map(chunk_layer_influence_bounds);
            } else {
                commands_dirty = true;
            }
        }
        self.dependency_free = self.layer_nodes.is_empty() && self.nonlocal_dependencies.is_empty();
        drop(chunk_profile);
        let plan_profile = crate::wgpu::start_cpu_scope("retained.materialize.plan_sync");
        let mut plain_topology_candidate =
            changes.topology_changed && removed_plain_leaves && !root_reorder_candidate;
        let mut topology_batch_updates = Vec::new();
        if plain_topology_candidate {
            for &id in &changes.changed_nodes {
                let Some(node) = scene.nodes.get(&id) else {
                    continue;
                };
                let Some(chunk) = self.chunks.get(&id) else {
                    plain_topology_candidate = false;
                    break;
                };
                let Some(parent) = node.parent else {
                    plain_topology_candidate = false;
                    break;
                };
                let Some(&batch) = self.container_batches.get(&parent) else {
                    plain_topology_candidate = false;
                    break;
                };
                if !matches!(node.kind, NodeKind::Scene { .. }) || !chunk.plain_fragment {
                    plain_topology_candidate = false;
                    break;
                }
                topology_batch_updates.push((id, batch));
            }
        }
        if plain_topology_candidate {
            self.node_batches.extend(topology_batch_updates);
        }
        let root_layer_add_candidate = changes
            .removed_nodes
            .is_empty()
            .then(|| {
                let mut layers = changes.changed_nodes.iter().filter(|id| {
                    scene.nodes.get(id).is_some_and(|node| {
                        matches!(node.kind, NodeKind::Layer(_))
                            && !self.layer_command_locations.contains_key(id)
                            && node.parent == Some(RetainedParent::content(scene.root))
                    })
                });
                let id = *layers.next()?;
                let is_tail = scene.nodes[&scene.root]
                    .content
                    .order
                    .last_key_value()
                    .is_some_and(|(_, tail)| *tail == id);
                if (!previous_dependency_free && !is_tail)
                    || layers.next().is_some()
                    || !changes
                        .changed_nodes
                        .iter()
                        .all(|changed| is_descendant_or_self(scene, *changed, id))
                {
                    return None;
                }
                Some(id)
            })
            .flatten();
        let nested_offscreen_add_candidate = root_layer_add_candidate
            .is_none()
            .then(|| self.nested_offscreen_add_candidate(scene, &changes))
            .flatten();
        let nested_offscreen_remove_candidate = root_layer_remove_candidate
            .is_none()
            .then(|| self.nested_offscreen_remove_candidate(scene, &changes))
            .flatten();
        let nested_offscreen_candidate =
            nested_offscreen_add_candidate.or(nested_offscreen_remove_candidate);
        let nested_offscreen_hierarchy_candidates = if nested_offscreen_candidate.is_none() {
            self.nested_offscreen_hierarchy_candidates(scene, &changes)
        } else {
            Vec::new()
        };
        let root_offscreen_reorder_candidate = nested_offscreen_hierarchy_candidates.is_empty()
            && changes.hierarchy_changed
            && changes.removed_nodes.is_empty()
            && !changes.changed_nodes.is_empty()
            && changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_some_and(|node| {
                    matches!(node.kind, NodeKind::Layer(_))
                        && node.parent == Some(RetainedParent::content(scene.root))
                        && self.layer_command_locations.contains_key(id)
                })
            })
            && scene.nodes[&scene.root]
                .content
                .values()
                .all(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)));

        if !changes.topology_changed && !self.dependency_free {
            delta_eligible = changes.changed_nodes.iter().all(|id| {
                scene.nodes.get(id).is_none_or(|node| {
                    matches!(node.kind, NodeKind::Group)
                        || position_plan_patches.contains(id)
                        || self.chunks.get(id).is_some_and(|chunk| {
                            self.raw_node_bounds.get(id).copied()
                                == Some(chunk.canvas.visual_bounds())
                        })
                })
            });
        }

        let compacted = self.arena_compactions() != compactions_before;
        if compacted {
            // Compaction remaps every live physical allocation in the affected arena. Re-encode
            // all cross-arena offsets once, then rebuild command draw references atomically.
            self.remap_all_chunks();
            commands_dirty = true;
            plan_dirty = true;
        }
        let position_plan_patched = plan_dirty
            && !unpatchable_plan_change
            && !compacted
            && !position_plan_patches.is_empty()
            && position_plan_patches
                .iter()
                .all(|&id| self.patch_scene_position_plan(id));
        if position_plan_patched {
            // The immutable plan Arc may still be owned by the renderer for the previous frame.
            // Advance the key so prepare selects this patched Arc without recompiling the scene.
            self.plan_cache_key = next_plan_cache_key();
            plan_dirty = false;
        }
        if scene_data_changed {
            self.sync_canvas_data(chunks_rebuilt, false);
        } else {
            Arc::make_mut(&mut self.canvas).buffer_changes = None;
        }
        if commands_dirty
            && (compacted
                || (changes.topology_changed
                    && !root_painter_update
                    && !root_reorder_candidate
                    && !plain_topology_candidate
                    && root_layer_add_candidate.is_none()
                    && root_layer_remove_candidate.is_none()
                    && nested_offscreen_candidate.is_none()
                    && nested_offscreen_hierarchy_candidates.is_empty()
                    && !root_offscreen_reorder_candidate))
        {
            self.rebuild_commands(scene);
            commands_dirty = false;
        }
        let root_layer_remove_patched = root_layer_remove_candidate
            .filter(|_| !compacted)
            .is_some_and(|id| self.remove_root_plan_fragment(id));
        if root_layer_remove_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let root_layer_plan_patched =
            root_layer_add_candidate
                .filter(|_| !compacted)
                .is_some_and(|id| {
                    let command_start = Arc::make_mut(&mut self.canvas).command_lists.len();
                    self.append_node_commands(scene, id, 0);
                    let command_end = Arc::make_mut(&mut self.canvas).command_lists.len();
                    self.append_root_plan_fragment(scene, id, command_start..command_end)
                });
        if root_layer_plan_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let nested_offscreen_plan_patched = nested_offscreen_candidate
            .filter(|_| !compacted)
            .is_some_and(|id| {
                if nested_offscreen_remove_candidate.is_some() {
                    for removed in &changes.removed_nodes {
                        self.scene_command_locations.remove(removed);
                        self.layer_command_locations.remove(removed);
                        self.node_batches.remove(removed);
                        self.painter_bases.remove(removed);
                        self.painter_parents.remove(removed);
                        self.container_batches
                            .retain(|parent, _| parent.node != *removed);
                    }
                }
                self.rebuild_offscreen_plan_fragment(scene, id)
            });
        if nested_offscreen_plan_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let nested_offscreen_hierarchy_patched = !nested_offscreen_hierarchy_candidates.is_empty()
            && !compacted
            && nested_offscreen_hierarchy_candidates
                .iter()
                .all(|&id| self.rebuild_offscreen_plan_fragment(scene, id));
        if nested_offscreen_hierarchy_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let root_offscreen_reorder_patched = root_offscreen_reorder_candidate
            && !compacted
            && self.reorder_root_offscreen_plan(scene);
        if root_offscreen_reorder_patched {
            commands_dirty = false;
            plan_compiled_during_update = true;
        }
        let layer_plan_patched = layer_update_candidate
            && !compacted
            && !layer_plan_patches.is_empty()
            && Arc::make_mut(&mut self.canvas)
                .compiled_plan
                .as_mut()
                .is_some_and(|plan| {
                    let plan = Arc::make_mut(plan);
                    for (id, old, new) in &layer_plan_patches {
                        if let Command::Layer { draw, .. } = old {
                            plan_layer_stack_changes
                                .extend(plan.layer_stack_ranges_for_draw(*draw));
                        }
                        if !plan.patch_retained_layer(*id, old, new) {
                            return false;
                        }
                    }
                    true
                });
        plan_layer_stack_changes = merge_index_ranges(plan_layer_stack_changes);
        plan_compiled_during_update |= layer_plan_patched;
        // Any command topology or layer parameter change invalidates the previous immutable
        // plan before painter metadata asks Canvas to compile. Clearing it only at the end would
        // let rebuild_painter_metadata copy batch membership from the stale plan.
        if plan_dirty
            && !layer_plan_patched
            && !root_layer_plan_patched
            && !root_layer_remove_patched
            && !nested_offscreen_plan_patched
            && !nested_offscreen_hierarchy_patched
            && !root_offscreen_reorder_patched
            && !plain_topology_candidate
            && !root_painter_update
            && !root_reorder_candidate
        {
            Arc::make_mut(&mut self.canvas).compiled_plan = None;
        }
        if scene_data_changed {
            if (root_painter_update || root_reorder_candidate || plain_topology_candidate)
                && !compacted
            {
                for id in &changes.removed_nodes {
                    self.painter_bases.remove(id);
                    self.painter_parents.remove(id);
                    self.node_batches.remove(id);
                }
                for &id in &changes.changed_nodes {
                    let Some(node) = scene.nodes.get(&id) else {
                        continue;
                    };
                    if !matches!(node.kind, NodeKind::Scene { .. }) {
                        continue;
                    }
                    let base = painter_path(scene, id);
                    let parent = node.parent.expect("retained scene leaf has parent");
                    self.painter_bases.insert(id, base);
                    self.painter_parents.insert(id, parent);
                    if self.dependency_free {
                        self.node_batches.entry(id).or_insert(0);
                    }
                }
                self.update_painter_metadata(&changes.changed_nodes);
            } else if root_layer_plan_patched
                || nested_offscreen_plan_patched
                || nested_offscreen_hierarchy_patched
                || root_offscreen_reorder_patched
            {
                for &id in &topology_changes.changed_nodes {
                    let Some(node) = scene.nodes.get(&id) else {
                        continue;
                    };
                    if matches!(node.kind, NodeKind::Scene { .. }) {
                        self.painter_bases.insert(id, painter_path(scene, id));
                        self.painter_parents
                            .insert(id, node.parent.expect("retained leaf has parent"));
                    }
                }
                self.update_painter_metadata(&topology_changes.changed_nodes);
            } else if root_layer_remove_patched {
                self.update_painter_metadata(&changes.changed_nodes);
            } else if (changes.topology_changed && !layer_plan_patched) || compacted {
                self.rebuild_painter_metadata(scene);
                plan_compiled_during_update = true;
            } else if layer_plan_patched || position_plan_patched {
                // Layer parameters and hidden layer geometry do not change leaf painter keys or
                // stable batch membership. The plan fragment above already references the new
                // hidden draw slot, so cloning the scene-wide metadata arrays would be wasted.
            } else {
                let stable_batches_remain = stable_batch_candidate
                    && !plan_dirty
                    && changes.changed_nodes.iter().all(|id| {
                        scene.nodes.get(id).is_none_or(|node| {
                            matches!(node.kind, NodeKind::Group)
                                || self
                                    .chunks
                                    .get(id)
                                    .is_some_and(|chunk| chunk.plain_fragment)
                        })
                    });
                if stable_batches_remain && flat_plan_had_draws && self.flat_plan_has_draws {
                    // Content-only updates with stable physical allocations cannot change painter
                    // paths or BatchIds. The old path rewrote and compared every changed draw,
                    // making an all-node revision pay a second O(changes) metadata walk after the
                    // chunks had already been patched.
                    plan_dirty = false;
                } else {
                    self.update_painter_metadata(&changes.changed_nodes);
                }
            }
        }
        if topology_delta_eligible
            && !root_layer_plan_patched
            && !root_layer_remove_patched
            && !nested_offscreen_plan_patched
            && !nested_offscreen_hierarchy_patched
            && !root_offscreen_reorder_patched
            && flat_plan_had_draws
            && self.flat_plan_has_draws
            && !compacted
        {
            commands_dirty = false;
            plan_dirty = false;
        }
        if (plain_topology_candidate || root_painter_update) && flat_plan_had_draws && !compacted {
            commands_dirty = false;
            plan_dirty = false;
            if let Some(frame) = &previous_frame {
                for &id in &changes.changed_nodes {
                    let Some(old) = frame.node_state(id) else {
                        continue;
                    };
                    let Some(chunk) = self.chunks.get(&id) else {
                        continue;
                    };
                    let new = self.influenced_bounds(scene, id, chunk.canvas.visual_bounds());
                    plain_topology_damage.push((id, old.bounds.union(new)));
                }
            }
        }
        let root_reorder_eligible = root_reorder_candidate && chunks_rebuilt == 0 && !compacted;
        if root_reorder_eligible {
            reorder_damage = self.root_reorder_damage(&old_painter_bases, &changes.changed_nodes);
            commands_dirty = false;
            plan_dirty = false;
        }
        if commands_dirty {
            self.rebuild_commands(scene);
        }
        if plan_dirty {
            self.plan_cache_key = next_plan_cache_key();
            if !plan_compiled_during_update {
                Arc::make_mut(&mut self.canvas).compiled_plan = None;
                self.refresh_compiled_plan();
                self.sync_stable_batches_from_plan();
            }
        }
        if let Some(buffer_changes) = &mut Arc::make_mut(&mut self.canvas).buffer_changes {
            buffer_changes.plan_structure_reused =
                layer_plan_patched || root_offscreen_reorder_patched;
            buffer_changes.plan_values_patched = position_plan_patched;
            buffer_changes.plan_layer_stack = if layer_plan_patched {
                plan_layer_stack_changes
            } else {
                Vec::new()
            };
            buffer_changes.filter_resources_changed = layer_plan_patched
                && layer_plan_patches.iter().any(|(_, old, new)| {
                    command_has_filter_resources(old) || command_has_filter_resources(new)
                });
            buffer_changes.plan_fragments_rebuilt = if position_plan_patched {
                position_plan_patches.len() as u32
            } else if layer_plan_patched {
                layer_plan_patches.len() as u32
            } else if root_layer_plan_patched {
                changes.changed_nodes.len() as u32
            } else if root_layer_remove_patched {
                changes.removed_nodes.len() as u32
            } else if nested_offscreen_plan_patched {
                if nested_offscreen_remove_candidate.is_some() {
                    changes.removed_nodes.len() as u32 + 1
                } else {
                    changes.changed_nodes.len() as u32 + 1
                }
            } else if nested_offscreen_hierarchy_patched {
                topology_changes.changed_nodes.len() as u32
                    + nested_offscreen_hierarchy_candidates.len() as u32
            } else if root_offscreen_reorder_patched {
                topology_changes.changed_nodes.len() as u32
            } else if plan_dirty {
                scene.nodes.len() as u32
            } else {
                0
            };
        }
        if changes.hierarchy_changed {
            self.refresh_root_fragment_membership(scene, &changes.changed_nodes);
        }
        drop(plan_profile);
        let frame_profile = crate::wgpu::start_cpu_scope("retained.materialize.frame");
        Arc::make_mut(&mut self.canvas).plan_cache_key = Some(self.plan_cache_key);
        let canvas = Arc::make_mut(&mut self.canvas);
        canvas.invalidated_bounds.clear();
        canvas.invalidate_all = changes.invalidate_all;
        for &rect in &changes.invalidated_rects {
            canvas.invalidate_rect(rect);
        }
        delta_eligible |= layer_plan_patched && layer_bounds_stable;
        let frame_patched = if delta_eligible {
            self.patch_frame_override(scene, previous_frame.clone(), &changes.changed_nodes);
            true
        } else {
            (topology_delta_eligible
                || plain_topology_candidate
                || root_painter_update
                || root_layer_plan_patched
                || root_layer_remove_patched
                || nested_offscreen_plan_patched
                || nested_offscreen_hierarchy_patched
                || root_offscreen_reorder_patched)
                && self.patch_topology_frame_override(
                    scene,
                    previous_frame.clone(),
                    topology_changes,
                    &plain_topology_damage,
                )
                || (root_reorder_eligible
                    && self.patch_topology_frame_override(
                        scene,
                        previous_frame.clone(),
                        &changes,
                        &reorder_damage,
                    ))
        };
        if !frame_patched {
            self.rebuild_frame_override(scene);
            self.rebuild_spatial_index();
        }
        drop(frame_profile);
        self.version = scene.version;
        scene_data_changed
    }

    fn rebuild_all(&mut self, scene: &RetainedScene) {
        self.scene_id = scene.id;
        self.version = scene.version;
        self.chunks.clear();
        self.arenas = MaterializedArenas::default();
        self.nested_cache = RetainedSceneCache::default();
        self.plan_cache_key = next_plan_cache_key();
        self.scene_command_locations.clear();
        self.resource_refs.clear();
        self.painter_bases.clear();
        self.painter_parents.clear();
        self.flat_plan_has_draws = false;
        self.node_bounds.clear();
        self.raw_node_bounds.clear();
        self.node_tiles.clear();
        self.node_batches.clear();
        self.container_batches.clear();
        self.root_plan_fragments.clear();
        self.layer_nodes = scene
            .nodes
            .iter()
            .filter_map(|(&id, node)| matches!(&node.kind, NodeKind::Layer(_)).then_some(id))
            .collect();
        self.nonlocal_dependencies.clear();
        self.canvas = Arc::new(Canvas::new_retained(
            scene.width,
            scene.height,
            scene.scale,
            scene.root,
        ));
        for (&id, node) in &scene.nodes {
            if !matches!(node.kind, NodeKind::Group) {
                self.rebuild_node(scene, id);
            }
        }
        self.dependency_free = self.layer_nodes.is_empty() && self.nonlocal_dependencies.is_empty();
        self.sync_canvas_data(self.chunks.len() as u32, true);
        self.rebuild_commands(scene);
        self.rebuild_painter_metadata(scene);
        self.index_root_plan_fragments(scene);
        Arc::make_mut(&mut self.canvas)
            .buffer_changes
            .as_mut()
            .expect("full sync records scene buffer changes")
            .plan_fragments_rebuilt = scene.nodes.len() as u32;
        Arc::make_mut(&mut self.canvas).plan_cache_key = Some(self.plan_cache_key);
        self.rebuild_frame_override(scene);
        self.rebuild_spatial_index();
    }

    /// Re-encodes one node and classifies whether its execution plan needs synchronization.
    fn rebuild_node(&mut self, scene: &RetainedScene, id: RetainedNodeId) -> NodeRebuild {
        let node = &scene.nodes[&id];
        let (source_canvas, position_bits) = scene_node_placement(node);
        let updated = {
            // Borrow the materializer fields independently so an existing chunk stays in its map
            // slot while encoding, arena remapping, and resource refcounts are updated.
            let Self {
                canvas,
                chunks,
                arenas,
                nested_cache,
                resource_refs,
                ..
            } = self;
            chunks.get_mut(&id).map(|chunk| {
                let position_only = chunk
                    .source_canvas
                    .as_ref()
                    .zip(source_canvas.as_ref())
                    .is_some_and(|(old, new)| Arc::ptr_eq(old, new))
                    && chunk.position_bits != position_bits
                    && source_canvas.is_some();
                Self::remove_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                let old_plan = chunk.plan_fingerprint;
                let old_lengths = SceneChunkLengths::from_canvas(&chunk.canvas);
                Self::encode_node_into(nested_cache, scene, node, &mut chunk.canvas);
                let encoded = &chunk.canvas;
                let new_lengths = SceneChunkLengths::from_canvas(encoded);
                let new_plan = encoded.execution_plan_fingerprint();
                let mut moved = false;
                if old_lengths.lines != 0 || new_lengths.lines != 0 {
                    moved |= arenas.lines.resize(chunk.lines, new_lengths.lines);
                }
                if old_lengths.paths != 0 || new_lengths.paths != 0 {
                    moved |= arenas.paths.resize(chunk.paths, new_lengths.paths);
                }
                if old_lengths.draws != 0 || new_lengths.draws != 0 {
                    moved |= arenas.draws.resize(chunk.draws, new_lengths.draws);
                }
                if old_lengths.brushes != 0 || new_lengths.brushes != 0 {
                    moved |= arenas.brushes.replace(chunk.brushes, &encoded.brush_blob);
                }
                if old_lengths.sdfs != 0 || new_lengths.sdfs != 0 {
                    moved |= arenas.sdfs.replace(chunk.sdfs, &encoded.sdf_blob);
                }
                if old_lengths.shadows != 0 || new_lengths.shadows != 0 {
                    moved |= arenas
                        .shadows
                        .replace(chunk.shadows, &encoded.sdf_shadow_blob);
                }
                moved |= Self::replace_glyphs(arenas, &mut chunk.glyphs, &encoded.text_glyphs);
                if old_lengths.runs != 0 || new_lengths.runs != 0 {
                    moved |= arenas.runs.resize(chunk.runs.unwrap(), new_lengths.runs);
                }
                if old_lengths.backdrops != 0 || new_lengths.backdrops != 0 {
                    moved |= arenas
                        .backdrops
                        .replace(chunk.backdrops, &vec![0; new_lengths.backdrops]);
                }
                if old_lengths.segments != 0 || new_lengths.segments != 0 {
                    moved |= arenas
                        .segments
                        .replace(chunk.segments, &vec![0; new_lengths.segments]);
                }
                chunk.generation = node.generation;
                chunk.source_canvas = source_canvas.clone();
                chunk.position_bits = position_bits;
                if old_plan != new_plan {
                    chunk.plain_fragment = is_plain_fragment(&chunk.canvas);
                }
                chunk.plan_fingerprint = new_plan;
                chunk.backdrop_dependencies = backdrop_dependencies(&chunk.canvas);
                Self::add_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                Self::remap_chunk_data(arenas, chunk);
                NodeRebuild {
                    plan_dirty: moved || old_plan != new_plan,
                    position_only: position_only && !moved,
                }
            })
        };
        if let Some(updated) = updated {
            self.refresh_nonlocal_dependency(id);
            return updated;
        }

        let encoded = Self::encode_node(&mut self.nested_cache, scene, node);
        let glyphs = Self::insert_glyphs(&mut self.arenas, &encoded.text_glyphs);
        let plan_fingerprint = encoded.execution_plan_fingerprint();
        let chunk = SceneChunk {
            generation: node.generation,
            source_canvas,
            position_bits,
            lines: self.arenas.lines.insert(&encoded.lines),
            paths: self.arenas.paths.insert(&encoded.path_records),
            draws: self.arenas.draws.insert(&encoded.draw_records),
            brushes: self.arenas.brushes.insert(&encoded.brush_blob),
            sdfs: self.arenas.sdfs.insert(&encoded.sdf_blob),
            shadows: self.arenas.shadows.insert(&encoded.sdf_shadow_blob),
            glyphs,
            runs: Some(self.arenas.runs.insert(&encoded.text_runs)),
            backdrops: self
                .arenas
                .backdrops
                .insert(&vec![0; encoded.backdrop_pool_capacity as usize]),
            segments: self
                .arenas
                .segments
                .insert(&vec![0; encoded.tile_cnt as usize]),
            plan_fingerprint,
            plain_fragment: is_plain_fragment(&encoded),
            backdrop_dependencies: backdrop_dependencies(&encoded),
            canvas: encoded,
        };
        Self::add_chunk_resources(
            &mut self.resource_refs,
            &mut self.canvas,
            &chunk.canvas.scene_images,
        );
        Self::remap_chunk_data(&mut self.arenas, &chunk);
        self.chunks.insert(id, chunk);
        self.refresh_nonlocal_dependency(id);
        NodeRebuild {
            plan_dirty: true,
            position_only: false,
        }
    }

    fn refresh_nonlocal_dependency(&mut self, id: RetainedNodeId) {
        if self
            .chunks
            .get(&id)
            .is_some_and(|chunk| !chunk.backdrop_dependencies.is_empty())
        {
            self.nonlocal_dependencies.insert(id);
        } else {
            self.nonlocal_dependencies.remove(&id);
        }
    }

    fn remap_chunk_data(arenas: &mut MaterializedArenas, chunk: &SceneChunk) {
        let line_base = if chunk.canvas.lines.is_empty() {
            0
        } else {
            arenas.lines.range(chunk.lines).start as u32
        };
        let path_base = if chunk.canvas.path_records.is_empty() {
            0
        } else {
            arenas.paths.range(chunk.paths).start as u32
        };
        let brush_base = if chunk.canvas.brush_blob.is_empty() {
            0
        } else {
            arenas.brushes.range(chunk.brushes).start as u32
        };
        let sdf_base = if chunk.canvas.sdf_blob.is_empty() {
            0
        } else {
            arenas.sdfs.range(chunk.sdfs).start as u32
        };
        let shadow_base = if chunk.canvas.sdf_shadow_blob.is_empty() {
            0
        } else {
            arenas.shadows.range(chunk.shadows).start as u32
        };
        let glyph_base = chunk.glyphs.map_or(0, |id| {
            arenas.glyphs.as_ref().unwrap().range(id).start as u32
        });
        let run_base = chunk
            .runs
            .filter(|_| !chunk.canvas.text_runs.is_empty())
            .map_or(0, |id| arenas.runs.range(id).start as u32);
        let backdrop_base = if chunk.canvas.backdrop_pool_capacity == 0 {
            0
        } else {
            arenas.backdrops.range(chunk.backdrops).start as u32
        };
        let segment_base = if chunk.canvas.tile_cnt == 0 {
            0
        } else {
            arenas.segments.range(chunk.segments).start as u32
        };

        if !chunk.canvas.lines.is_empty() {
            arenas
                .lines
                .write_mapped(chunk.lines, &chunk.canvas.lines, |mut line| {
                    line.path_id = line.path_id.saturating_add(path_base);
                    line
                });
        }

        if !chunk.canvas.path_records.is_empty() {
            arenas
                .paths
                .write_mapped(chunk.paths, &chunk.canvas.path_records, |mut path| {
                    path.path_id = path.path_id.saturating_add(path_base);
                    path.line_start = path.line_start.saturating_add(line_base);
                    path.data_offset = path.data_offset.saturating_add(backdrop_base);
                    path.segment_start = path.segment_start.saturating_add(segment_base);
                    path
                });
        }

        if !chunk.canvas.draw_records.is_empty() {
            arenas
                .draws
                .write_mapped(chunk.draws, &chunk.canvas.draw_records, |mut draw| {
                    if draw.path_id != DrawRecord::NONE {
                        draw.path_id = draw.path_id.saturating_add(path_base);
                    }
                    if draw.glyph_run_id != DrawRecord::NONE {
                        draw.glyph_run_id = draw.glyph_run_id.saturating_add(run_base);
                    }
                    if draw.brush_offset != DrawRecord::NONE {
                        draw.brush_offset = draw.brush_offset.saturating_add(brush_base);
                    }
                    if draw.sdf_offset != DrawRecord::NONE {
                        draw.sdf_offset = draw.sdf_offset.saturating_add(sdf_base);
                    }
                    if draw.sdf_shadow_offset != DrawRecord::NONE {
                        draw.sdf_shadow_offset = draw.sdf_shadow_offset.saturating_add(shadow_base);
                    }
                    draw
                });
        }
        if let Some(run_allocation) = chunk.runs.filter(|_| !chunk.canvas.text_runs.is_empty()) {
            arenas
                .runs
                .write_mapped(run_allocation, &chunk.canvas.text_runs, |run| TextRun {
                    glyph_start: run.glyph_start.saturating_add(glyph_base),
                    glyph_count: run.glyph_count,
                });
        }
    }

    fn encode_node(
        nested_cache: &mut RetainedSceneCache,
        scene: &RetainedScene,
        node: &SceneNode,
    ) -> Canvas {
        let mut canvas = Canvas::new(scene.width, scene.height, scene.scale);
        Self::encode_node_into(nested_cache, scene, node, &mut canvas);
        canvas
    }

    fn encode_node_into(
        nested_cache: &mut RetainedSceneCache,
        scene: &RetainedScene,
        node: &SceneNode,
        canvas: &mut Canvas,
    ) {
        canvas.reset();
        match &node.kind {
            NodeKind::Scene {
                canvas: child,
                position,
            } => {
                let materialized = if child.has_retained_scenes() {
                    nested_cache.materialize_snapshot_shared(child)
                } else {
                    child.clone()
                };
                canvas.append(&materialized, *position);
            }
            NodeKind::Layer(layer) => {
                match layer {
                    RetainedLayerDescriptor::ClipPath {
                        path,
                        transform,
                        rule,
                        tolerance,
                    } => canvas.push_clip_layer(path.clone(), *transform, *rule, *tolerance),
                    RetainedLayerDescriptor::ClipSdf(sdf) => canvas.push_clip_sdf_layer(*sdf),
                    RetainedLayerDescriptor::Isolate {
                        path,
                        transform,
                        tolerance,
                    } => canvas.push_isolate_layer(path.clone(), *transform, *tolerance),
                    RetainedLayerDescriptor::Opacity {
                        path,
                        transform,
                        tolerance,
                        opacity,
                    } => canvas.push_opacity_layer(path.clone(), *transform, *tolerance, *opacity),
                    RetainedLayerDescriptor::Blend {
                        path,
                        transform,
                        tolerance,
                        mix,
                        compose,
                    } => canvas.push_blend_layer(
                        path.clone(),
                        *transform,
                        *tolerance,
                        *mix,
                        *compose,
                    ),
                    RetainedLayerDescriptor::Filter {
                        filter,
                        sample_region,
                    } => canvas.push_filter_layer(filter.clone(), sample_region.clone()),
                    RetainedLayerDescriptor::Backdrop {
                        filter,
                        sample_region,
                    } => canvas.push_backdrop_layer(filter.clone(), sample_region.clone()),
                    RetainedLayerDescriptor::Mask(mask) => {
                        canvas.push_mask_layer(
                            Canvas::new(scene.width, scene.height, scene.scale),
                            mask.clone(),
                        );
                    }
                }
                canvas.pop_layer();
            }
            NodeKind::Group => {}
        }
    }

    fn insert_glyphs(
        arenas: &mut MaterializedArenas,
        glyphs: &[CanvasGlyph],
    ) -> Option<ArenaAllocation> {
        let first = glyphs.first().copied()?;
        let arena = arenas.glyphs.get_or_insert_with(|| SceneArena::new(first));
        Some(arena.insert(glyphs))
    }

    fn replace_glyphs(
        arenas: &mut MaterializedArenas,
        allocation: &mut Option<ArenaAllocation>,
        glyphs: &[CanvasGlyph],
    ) -> bool {
        match (*allocation, glyphs.first().copied()) {
            (Some(id), Some(first)) => arenas
                .glyphs
                .get_or_insert_with(|| SceneArena::new(first))
                .replace(id, glyphs),
            (Some(id), None) => {
                arenas.glyphs.as_mut().unwrap().remove(id);
                *allocation = None;
                true
            }
            (None, Some(_)) => {
                *allocation = Self::insert_glyphs(arenas, glyphs);
                true
            }
            (None, None) => false,
        }
    }

    fn remove_chunk(&mut self, chunk: SceneChunk) {
        Self::remove_chunk_resources(
            &mut self.resource_refs,
            &mut self.canvas,
            &chunk.canvas.scene_images,
        );
        self.arenas.lines.remove(chunk.lines);
        self.arenas.paths.remove(chunk.paths);
        self.arenas.draws.remove(chunk.draws);
        self.arenas.brushes.remove(chunk.brushes);
        self.arenas.sdfs.remove(chunk.sdfs);
        self.arenas.shadows.remove(chunk.shadows);
        if let Some(glyphs) = chunk.glyphs {
            self.arenas.glyphs.as_mut().unwrap().remove(glyphs);
        }
        self.arenas.runs.remove(chunk.runs.unwrap());
        self.arenas.backdrops.remove(chunk.backdrops);
        self.arenas.segments.remove(chunk.segments);
    }

    fn add_chunk_resources(
        resource_refs: &mut HashMap<ImageKey, (Arc<Image>, usize)>,
        canvas: &mut Arc<Canvas>,
        resources: &ImageResourceStore,
    ) {
        for (key, image) in resources.iter() {
            let entry = resource_refs
                .entry(key)
                .or_insert_with(|| (image.clone(), 0));
            entry.0 = image.clone();
            entry.1 += 1;
            Arc::make_mut(canvas)
                .scene_images
                .insert(key, image.clone());
        }
    }

    fn remove_chunk_resources(
        resource_refs: &mut HashMap<ImageKey, (Arc<Image>, usize)>,
        canvas: &mut Arc<Canvas>,
        resources: &ImageResourceStore,
    ) {
        for (key, _) in resources.iter() {
            let remove = resource_refs.get_mut(&key).is_some_and(|(_, count)| {
                *count -= 1;
                *count == 0
            });
            if remove {
                resource_refs.remove(&key);
                Arc::make_mut(canvas).scene_images.remove(key);
            }
        }
    }

    fn arena_compactions(&self) -> u64 {
        self.arenas.lines.compactions()
            + self.arenas.paths.compactions()
            + self.arenas.draws.compactions()
            + self.arenas.brushes.compactions()
            + self.arenas.sdfs.compactions()
            + self.arenas.shadows.compactions()
            + self
                .arenas
                .glyphs
                .as_ref()
                .map_or(0, SceneArena::compactions)
            + self.arenas.runs.compactions()
            + self.arenas.backdrops.compactions()
            + self.arenas.segments.compactions()
    }

    fn remap_all_chunks(&mut self) {
        let Self { chunks, arenas, .. } = self;
        for chunk in chunks.values() {
            Self::remap_chunk_data(arenas, chunk);
        }
    }

    fn sync_canvas_data(&mut self, chunks_rebuilt: u32, full_scene_sync: bool) {
        let (arena_live_bytes, arena_capacity_bytes) = self.arena_usage();
        let arena_fragmentation = if arena_capacity_bytes == 0 {
            0.0
        } else {
            1.0 - arena_live_bytes as f32 / arena_capacity_bytes as f32
        };
        let arena_compactions = self.arena_compactions();
        let canvas = Arc::make_mut(&mut self.canvas);
        let lines = sync_arena(&mut canvas.lines, &mut self.arenas.lines, Line::default());
        let paths = sync_arena(
            &mut canvas.path_records,
            &mut self.arenas.paths,
            PathRecord::default(),
        );
        let draws = sync_arena(
            &mut canvas.draw_records,
            &mut self.arenas.draws,
            inactive_draw(),
        );
        let brushes = sync_arena(&mut canvas.brush_blob, &mut self.arenas.brushes, 0);
        let sdfs = sync_arena(&mut canvas.sdf_blob, &mut self.arenas.sdfs, 0);
        let shadows = sync_arena(&mut canvas.sdf_shadow_blob, &mut self.arenas.shadows, 0);
        let glyphs = if let Some(glyphs) = &mut self.arenas.glyphs {
            let vacant = glyphs
                .values()
                .first()
                .copied()
                .expect("glyph arena is initialized by a glyph");
            sync_arena(&mut canvas.text_glyphs, glyphs, vacant)
        } else {
            canvas.text_glyphs.clear();
            Vec::new()
        };
        let text_runs = sync_arena(
            &mut canvas.text_runs,
            &mut self.arenas.runs,
            TextRun {
                glyph_start: 0,
                glyph_count: 0,
            },
        );
        canvas.path_cnt = self.arenas.paths.values().len() as u32;
        canvas.backdrop_pool_capacity = self.arenas.backdrops.values().len() as u32;
        canvas.tile_cnt = self.arenas.segments.values().len() as u32;
        let cpu_copied_bytes = range_bytes::<Line>(&lines)
            + range_bytes::<PathRecord>(&paths)
            + range_bytes::<DrawRecord>(&draws)
            + range_bytes::<u32>(&brushes)
            + range_bytes::<u32>(&sdfs)
            + range_bytes::<u32>(&shadows)
            + range_bytes::<CanvasGlyph>(&glyphs)
            + range_bytes::<TextRun>(&text_runs);
        canvas.buffer_changes = Some(SceneBufferChanges {
            lines,
            paths,
            draws,
            brushes,
            sdfs,
            shadows,
            glyphs,
            text_runs,
            chunks_rebuilt,
            plan_fragments_rebuilt: 0,
            full_scene_sync,
            cpu_copied_bytes,
            painter: Vec::new(),
            plan_structure_reused: false,
            plan_values_patched: false,
            plan_layer_stack: Vec::new(),
            filter_resources_changed: false,
            arena_live_bytes,
            arena_capacity_bytes,
            arena_fragmentation,
            arena_compactions,
        });
    }

    fn arena_usage(&self) -> (u64, u64) {
        let mut live = 0;
        let mut capacity = 0;
        macro_rules! add {
            ($arena:expr, $ty:ty) => {{
                live += ($arena.live_len() * std::mem::size_of::<$ty>()) as u64;
                capacity += ($arena.values().len() * std::mem::size_of::<$ty>()) as u64;
            }};
        }
        add!(self.arenas.lines, Line);
        add!(self.arenas.paths, PathRecord);
        add!(self.arenas.draws, DrawRecord);
        add!(self.arenas.brushes, u32);
        add!(self.arenas.sdfs, u32);
        add!(self.arenas.shadows, u32);
        if let Some(glyphs) = &self.arenas.glyphs {
            add!(glyphs, CanvasGlyph);
        }
        add!(self.arenas.runs, TextRun);
        add!(self.arenas.backdrops, u32);
        add!(self.arenas.segments, u32);
        (live, capacity)
    }

    fn rebuild_commands(&mut self, scene: &RetainedScene) {
        let canvas = Arc::make_mut(&mut self.canvas);
        canvas.compiled_plan = None;
        canvas.command_lists.clear();
        canvas.command_lists.push(CommandList::default());
        canvas.root_commands = 0;
        canvas.command_stack.clear();
        canvas.command_stack.push(0);
        canvas.layer_stack.clear();
        canvas.retained_root = Some(scene.root);
        self.scene_command_locations.clear();
        self.layer_command_locations.clear();
        self.root_plan_fragments.clear();
        self.append_children(scene, scene.root, RetainedChildBranch::Content, 0);
    }

    fn append_children(
        &mut self,
        scene: &RetainedScene,
        parent: RetainedNodeId,
        branch: RetainedChildBranch,
        target: usize,
    ) {
        let children = scene.nodes[&parent]
            .children(branch)
            .expect("validated retained branch")
            .values()
            .copied()
            .collect::<Vec<_>>();
        for child in children {
            self.append_node_commands(scene, child, target);
        }
    }

    fn append_node_commands(&mut self, scene: &RetainedScene, id: RetainedNodeId, target: usize) {
        let node = &scene.nodes[&id];
        match &node.kind {
            NodeKind::Group => {
                self.append_children(scene, id, RetainedChildBranch::Content, target)
            }
            NodeKind::Scene { .. } => {
                let (children, fragment_start, fragment_count) =
                    self.install_chunk_commands(id, None);
                let command_index = Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::MaterializedRetainedScene {
                        id,
                        revision: SceneRevision::new(node.generation),
                        children,
                    });
                self.scene_command_locations.insert(
                    id,
                    SceneCommandLocation {
                        parent_list: target,
                        command_index,
                        fragment_start,
                        fragment_count,
                    },
                );
            }
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let content = self.push_command_list();
                let mask_commands = self.push_command_list();
                self.append_children(scene, id, RetainedChildBranch::Content, content);
                self.append_children(scene, id, RetainedChildBranch::Mask, mask_commands);
                let command_index = Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::MaskLayer {
                        retained: Some(RetainedLayerKey::new(
                            id,
                            SceneRevision::new(node.generation),
                        )),
                        layer: mask.clone(),
                        content,
                        mask: mask_commands,
                    });
                self.layer_command_locations.insert(
                    id,
                    LayerCommandLocation {
                        parent_list: target,
                        command_index,
                    },
                );
            }
            NodeKind::Layer(_) => {
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let root = &chunk.canvas.command_lists[chunk.canvas.root_commands];
                let template = root
                    .commands
                    .first()
                    .expect("layer chunk has one root command")
                    .clone();
                let has_draws = !chunk.canvas.draw_records.is_empty();
                let children = self.push_command_list();
                self.append_children(scene, id, RetainedChildBranch::Content, children);
                let Command::Layer { draw, layer, .. } = template else {
                    unreachable!("non-mask layer chunk has a layer command")
                };
                let draw = if !has_draws { 0 } else { draw_base + draw };
                let command_index = Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .len();
                Arc::make_mut(&mut self.canvas).command_lists[target]
                    .commands
                    .push(Command::Layer {
                        retained: Some(RetainedLayerKey::new(
                            id,
                            SceneRevision::new(node.generation),
                        )),
                        draw,
                        layer,
                        children,
                    });
                self.layer_command_locations.insert(
                    id,
                    LayerCommandLocation {
                        parent_list: target,
                        command_index,
                    },
                );
            }
        }
    }

    fn install_chunk_commands(
        &mut self,
        id: RetainedNodeId,
        reuse: Option<std::ops::Range<usize>>,
    ) -> (usize, usize, usize) {
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        let list_count = chunk.canvas.command_lists.len();
        let list_base = reuse.filter(|range| range.len() == list_count).map_or_else(
            || Arc::make_mut(&mut self.canvas).command_lists.len(),
            |range| range.start,
        );
        let lists = chunk
            .canvas
            .command_lists
            .iter()
            .map(|list| CommandList {
                commands: list
                    .commands
                    .iter()
                    .cloned()
                    .map(|command| Canvas::remap_command(command, draw_base, list_base))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let command_lists = &mut Arc::make_mut(&mut self.canvas).command_lists;
        if list_base == command_lists.len() {
            command_lists.extend(lists);
        } else {
            command_lists[list_base..list_base + list_count].clone_from_slice(&lists);
        }
        (
            list_base + chunk.canvas.root_commands,
            list_base,
            list_count,
        )
    }

    fn patch_scene_commands(&mut self, scene: &RetainedScene, id: RetainedNodeId) {
        let old = self.scene_command_locations[&id].clone();
        let (children, fragment_start, fragment_count) = self.install_chunk_commands(
            id,
            Some(old.fragment_start..old.fragment_start + old.fragment_count),
        );
        Arc::make_mut(&mut self.canvas).command_lists[old.parent_list].commands
            [old.command_index] = Command::MaterializedRetainedScene {
            id,
            revision: SceneRevision::new(scene.nodes[&id].generation),
            children,
        };
        self.scene_command_locations.insert(
            id,
            SceneCommandLocation {
                fragment_start,
                fragment_count,
                ..old
            },
        );
    }

    /// Patches translated offscreen descriptors owned by one otherwise unchanged scene leaf.
    /// Draw records and stable batch membership live in arenas and do not need plan rebuilding.
    fn patch_scene_position_plan(&mut self, id: RetainedNodeId) -> bool {
        let location = &self.scene_command_locations[&id];
        let command = Arc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index]
            .clone();
        let temporary = self.push_command_list();
        Arc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .push(command);
        let fragment = self.canvas.compile(temporary);
        Arc::make_mut(&mut self.canvas).command_lists.pop();
        Arc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_mut()
            .is_some_and(|plan| Arc::make_mut(plan).patch_retained_scene_position(id, &fragment))
    }

    /// Replaces one retained layer fragment without walking or rewriting unrelated siblings.
    /// Child command lists remain stable, including the two independent mask branches.
    fn patch_layer_command(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> (Command, Command) {
        let location = self.layer_command_locations[&id];
        let old = Arc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index]
            .clone();
        let generation = SceneRevision::new(scene.nodes[&id].generation);
        let command = match &scene.nodes[&id].kind {
            NodeKind::Layer(RetainedLayerDescriptor::Mask(mask)) => {
                let (content, mask_commands) = match old.clone() {
                    Command::MaskLayer { content, mask, .. } => (content, mask),
                    Command::Layer { children, .. } => (children, self.push_command_list()),
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                Command::MaskLayer {
                    retained: Some(RetainedLayerKey::new(id, generation)),
                    layer: mask.clone(),
                    content,
                    mask: mask_commands,
                }
            }
            NodeKind::Layer(_) => {
                let children = match old.clone() {
                    Command::Layer { children, .. } => children,
                    Command::MaskLayer { content, .. } => content,
                    _ => unreachable!("retained layer location must reference a layer command"),
                };
                let chunk = &self.chunks[&id];
                let draw_base = self.arenas.draws.range(chunk.draws).start;
                let template =
                    chunk.canvas.command_lists[chunk.canvas.root_commands].commands[0].clone();
                let Command::Layer { draw, layer, .. } = template else {
                    unreachable!("non-mask retained layer chunk has one layer command")
                };
                Command::Layer {
                    retained: Some(RetainedLayerKey::new(id, generation)),
                    draw: draw_base + draw,
                    layer,
                    children,
                }
            }
            _ => unreachable!("changed layer set only contains retained layers"),
        };
        Arc::make_mut(&mut self.canvas).command_lists[location.parent_list].commands
            [location.command_index] = command.clone();
        (old, command)
    }

    fn nested_offscreen_add_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed
            || !changes.removed_nodes.is_empty()
            || !changes.changed_nodes.iter().any(|id| {
                scene
                    .nodes
                    .get(id)
                    .is_some_and(|node| matches!(node.kind, NodeKind::Layer(_)))
                    && !self.layer_command_locations.contains_key(id)
            })
        {
            return None;
        }
        let plan = self.canvas.compiled_plan.as_ref()?;
        let mut candidate = None;
        for &id in &changes.changed_nodes {
            if self.layer_command_locations.contains_key(&id)
                || self.scene_command_locations.contains_key(&id)
            {
                return None;
            }
            let mut parent = scene.nodes.get(&id)?.parent?.node;
            let ancestor = loop {
                if self.layer_command_locations.contains_key(&parent)
                    && plan.contains_retained_offscreen(parent)
                {
                    break parent;
                }
                parent = scene.nodes.get(&parent)?.parent?.node;
            };
            if candidate.is_some_and(|candidate| candidate != ancestor) {
                return None;
            }
            candidate = Some(ancestor);
        }
        candidate
    }

    fn nested_offscreen_remove_candidate(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Option<RetainedNodeId> {
        if !changes.topology_changed || changes.removed_nodes.is_empty() {
            return None;
        }
        let plan = self.canvas.compiled_plan.as_ref()?;
        let mut removed_layers = changes.removed_nodes.iter().filter(|id| {
            // Removed chunks have already been released by the time candidates are selected.
            // Layer command ownership is the stable pre-removal type/index needed here.
            self.layer_command_locations.contains_key(id)
        });
        let first = *removed_layers.next()?;
        let surviving_ancestor = |mut node| {
            let mut ancestor = plan.retained_offscreen_ancestor_of(node)?;
            while changes.removed_nodes.contains(&ancestor) {
                node = ancestor;
                ancestor = plan.retained_offscreen_ancestor_of(node)?;
            }
            scene.nodes.contains_key(&ancestor).then_some(ancestor)
        };
        let candidate = surviving_ancestor(first)?;
        removed_layers
            .all(|&layer| surviving_ancestor(layer) == Some(candidate))
            .then_some(candidate)
    }

    fn nested_offscreen_hierarchy_candidates(
        &self,
        scene: &RetainedScene,
        changes: &SceneChangeSet,
    ) -> Vec<RetainedNodeId> {
        if !changes.hierarchy_changed || !changes.removed_nodes.is_empty() {
            return Vec::new();
        }
        let Some(plan) = self.canvas.compiled_plan.as_ref() else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                return Vec::new();
            };
            if !matches!(node.kind, NodeKind::Layer(_))
                || !self.layer_command_locations.contains_key(&id)
            {
                return Vec::new();
            }
            let Some(old) = plan.retained_offscreen_ancestor_of(id) else {
                return Vec::new();
            };
            let mut parent = node.parent.map(|parent| parent.node);
            let new = loop {
                let Some(candidate) = parent else {
                    return Vec::new();
                };
                if self.layer_command_locations.contains_key(&candidate)
                    && plan.contains_retained_offscreen(candidate)
                {
                    break candidate;
                }
                parent = scene
                    .nodes
                    .get(&candidate)
                    .and_then(|node| node.parent.map(|p| p.node));
            };
            for candidate in [old, new] {
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    fn reorder_root_offscreen_plan(&mut self, scene: &RetainedScene) -> bool {
        let order = scene.nodes[&scene.root]
            .content
            .values()
            .copied()
            .collect::<Vec<_>>();
        let canvas = Arc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_mut() else {
            return false;
        };
        if !Arc::make_mut(plan).reorder_root_offscreen(&order) {
            return false;
        }

        let mut commands = HashMap::with_capacity_and_hasher(
            canvas.command_lists[0].commands.len(),
            Default::default(),
        );
        for command in &canvas.command_lists[0].commands {
            let id = match command {
                Command::Layer {
                    retained: Some(key),
                    ..
                }
                | Command::MaskLayer {
                    retained: Some(key),
                    ..
                } => key.id,
                _ => return false,
            };
            commands.insert(id, command.clone());
        }
        let Some(reordered) = order
            .iter()
            .map(|id| commands.remove(id))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if !commands.is_empty() {
            return false;
        }
        canvas.command_lists[0].commands = reordered;
        for (command_index, &id) in order.iter().enumerate() {
            self.layer_command_locations.insert(
                id,
                LayerCommandLocation {
                    parent_list: 0,
                    command_index,
                },
            );
        }
        self.root_plan_fragments.clear();
        true
    }

    /// Recompiles only one retained offscreen ancestor and installs its new command/plan
    /// fragment. Unchanged branch owners keep their BatchIds; new nested branches receive fresh
    /// IDs without rewriting unrelated root operations.
    fn rebuild_offscreen_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> bool {
        let old_location = self.layer_command_locations[&id];
        let temporary = self.push_command_list();
        self.append_node_commands(scene, id, temporary);
        let command = Arc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .first()
            .cloned()
            .expect("offscreen fragment root command");
        let fragment = Arc::make_mut(&mut self.canvas).compile(temporary);
        let Some(plan) = Arc::make_mut(&mut self.canvas).compiled_plan.as_mut() else {
            return false;
        };
        let Some(draw_batches) =
            Arc::make_mut(plan).replace_retained_offscreen_fragment(id, fragment)
        else {
            return false;
        };
        Arc::make_mut(&mut self.canvas).command_lists[old_location.parent_list].commands
            [old_location.command_index] = command;
        self.layer_command_locations.insert(id, old_location);

        let physical_batches = draw_batches.into_iter().collect::<HashMap<_, _>>();
        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        for &node in &nodes {
            if !matches!(scene.nodes[&node].kind, NodeKind::Scene { .. }) {
                continue;
            }
            let physical = self.node_physical_draws(node);
            let mut batches = physical
                .iter()
                .filter_map(|draw| physical_batches.get(draw).copied());
            if let Some(batch) = batches.next()
                && batches.all(|candidate| candidate == batch)
            {
                self.node_batches.insert(node, batch);
            }
        }
        let subtree = nodes.into_iter().collect::<HashSet<_>>();
        self.container_batches
            .retain(|parent, _| !subtree.contains(&parent.node));
        collect_container_batches(scene, id, &self.node_batches, &mut self.container_batches);
        let retained_batches = Arc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .unwrap()
            .retained_batch_ids
            .iter()
            .filter(|(owner, _)| subtree.contains(&owner.node))
            .map(|(owner, &batch)| {
                (
                    RetainedParent {
                        node: owner.node,
                        branch: match owner.branch {
                            RetainedBatchBranch::Content => RetainedChildBranch::Content,
                            RetainedBatchBranch::Mask => RetainedChildBranch::Mask,
                        },
                    },
                    batch,
                )
            })
            .collect::<Vec<_>>();
        self.container_batches.extend(retained_batches);
        true
    }

    /// Installs one newly appended root-layer subtree as an independent persistent plan
    /// fragment. Existing root batches and their large draw lists remain shared.
    fn append_root_plan_fragment(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        command_lists: std::ops::Range<usize>,
    ) -> bool {
        let location = self.layer_command_locations[&id];
        if location.parent_list != 0 {
            return false;
        }
        let command = Arc::make_mut(&mut self.canvas).command_lists[0].commands
            [location.command_index]
            .clone();
        let temporary = self.push_command_list();
        Arc::make_mut(&mut self.canvas).command_lists[temporary]
            .commands
            .push(command);
        let fragment = Arc::make_mut(&mut self.canvas).compile(temporary);
        let _ = Arc::make_mut(&mut self.canvas).command_lists.pop();

        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        let node_set = nodes.iter().copied().collect::<HashSet<_>>();
        let layer_bounds = chunk_layer_influence_bounds(&self.chunks[&id]);
        let layer_painter = painter_path(scene, id);
        // Only later leaves whose pixels intersect this layer can cross the new execution
        // boundary. Querying the retained tile index makes the work proportional to affected
        // pixels instead of walking every later sibling in the scene.
        let reassigned_batches = self
            .spatial_candidates(layer_bounds)
            .into_iter()
            .filter_map(|node| {
                if !matches!(scene.nodes.get(&node)?.kind, NodeKind::Scene { .. })
                    || self.painter_bases.get(&node)?.as_ref() <= layer_painter.as_ref()
                {
                    return None;
                }
                let bounds = self.node_bounds.get(&node)?;
                (!bounds.intersect(layer_bounds).is_empty())
                    .then(|| {
                        self.node_batches
                            .get(&node)
                            .copied()
                            .map(|batch| (node, batch))
                    })
                    .flatten()
            })
            .collect::<HashMap<_, _>>();
        let reassigned_draws = reassigned_batches
            .keys()
            .flat_map(|id| self.node_physical_draws(*id))
            .collect::<Vec<_>>();
        let mut moved_per_batch = HashMap::<u32, usize>::default();
        for (&node, &batch) in &reassigned_batches {
            *moved_per_batch.entry(batch).or_default() += self.node_physical_draws(node).len();
        }
        let Some(plan) = Arc::make_mut(&mut self.canvas).compiled_plan.as_mut() else {
            return false;
        };
        let plan = Arc::make_mut(plan);
        let removed_batch = (moved_per_batch.len() == 1)
            .then(|| moved_per_batch.into_iter().next().unwrap())
            .and_then(|(batch, moved)| plan.remove_batch_if_all_moved(batch, moved));
        let (mut ops, draw_batches) = plan.append_fragment(fragment);
        let retained_batches = plan
            .retained_batch_ids
            .iter()
            .filter(|(owner, _)| node_set.contains(&owner.node))
            .map(|(owner, &batch)| {
                (
                    RetainedParent {
                        node: owner.node,
                        branch: match owner.branch {
                            RetainedBatchBranch::Content => RetainedChildBranch::Content,
                            RetainedBatchBranch::Mask => RetainedChildBranch::Mask,
                        },
                    },
                    batch,
                )
            })
            .collect::<Vec<_>>();
        let after_batch = plan.append_plain_batch(reassigned_draws);
        ops.end = plan.ops.len();
        if let Some(batch) = after_batch {
            for &node in reassigned_batches.keys() {
                self.node_batches.insert(node, batch);
            }
            self.update_painter_metadata(
                &reassigned_batches.keys().copied().collect::<HashSet<_>>(),
            );
        }
        let physical_batches = draw_batches.into_iter().collect::<HashMap<_, _>>();
        for &node_id in &nodes {
            if !matches!(scene.nodes[&node_id].kind, NodeKind::Scene { .. }) {
                continue;
            }
            let physical = self.node_physical_draws(node_id);
            let mut batches = physical
                .iter()
                .filter_map(|draw| physical_batches.get(draw).copied());
            if let Some(batch) = batches.next()
                && batches.all(|candidate| candidate == batch)
            {
                self.node_batches.insert(node_id, batch);
            }
        }
        collect_container_batches(scene, id, &self.node_batches, &mut self.container_batches);
        self.container_batches.extend(retained_batches);
        self.root_plan_fragments.insert(
            id,
            RootPlanFragment {
                ops,
                command_lists,
                nodes: node_set,
                reassigned_batches,
                removed_batch,
            },
        );
        true
    }

    fn remove_root_plan_fragment(&mut self, id: RetainedNodeId) -> bool {
        let Some(fragment) = self.root_plan_fragments.remove(&id) else {
            return false;
        };
        let location = self.layer_command_locations[&id];
        let canvas = Arc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_mut() else {
            self.root_plan_fragments.insert(id, fragment);
            return false;
        };
        if location.parent_list != 0 || !Arc::make_mut(plan).remove_fragment(fragment.ops.clone()) {
            self.root_plan_fragments.insert(id, fragment);
            return false;
        }
        let removed_ops = fragment.ops.clone();
        for other in self.root_plan_fragments.values_mut() {
            if other.ops.start >= removed_ops.end {
                other.ops.start -= removed_ops.len();
                other.ops.end -= removed_ops.len();
            }
        }
        let root_commands = &mut canvas.command_lists[0].commands;
        let removed_tail_command = location.command_index + 1 == root_commands.len();
        root_commands.remove(location.command_index);
        // Appended root fragments are normally removed from the tail. In that case no command
        // location can shift, so scanning every retained leaf would turn a two-node removal into
        // O(scene nodes) work. Non-tail removal still repairs every affected stable location.
        if !removed_tail_command {
            for command_location in self.scene_command_locations.values_mut() {
                if command_location.parent_list == 0
                    && command_location.command_index > location.command_index
                {
                    command_location.command_index -= 1;
                }
            }
            for layer_location in self.layer_command_locations.values_mut() {
                if layer_location.parent_list == 0
                    && layer_location.command_index > location.command_index
                {
                    layer_location.command_index -= 1;
                }
            }
        }
        if !fragment.command_lists.is_empty()
            && fragment.command_lists.end == canvas.command_lists.len()
        {
            canvas.command_lists.truncate(fragment.command_lists.start);
        }
        if let Some((index, op)) = fragment.removed_batch {
            Arc::make_mut(plan).restore_removed_batch(index, op);
            for other in self.root_plan_fragments.values_mut() {
                if other.ops.start >= index {
                    other.ops.start += 1;
                    other.ops.end += 1;
                }
            }
        }
        let restored = fragment.reassigned_batches;
        for (&node, &batch) in &restored {
            self.node_batches.insert(node, batch);
        }
        for node in fragment.nodes {
            self.scene_command_locations.remove(&node);
            self.layer_command_locations.remove(&node);
            self.node_batches.remove(&node);
            self.painter_bases.remove(&node);
            self.painter_parents.remove(&node);
            self.container_batches
                .retain(|parent, _| parent.node != node);
        }
        self.update_painter_metadata(&restored.keys().copied().collect());
        true
    }

    fn refresh_root_fragment_membership(
        &mut self,
        scene: &RetainedScene,
        changed: &HashSet<RetainedNodeId>,
    ) {
        for (&root, fragment) in &mut self.root_plan_fragments {
            fragment.nodes.retain(|node| {
                scene.nodes.contains_key(node) && is_descendant_or_self(scene, *node, root)
            });
            fragment.nodes.extend(
                changed
                    .iter()
                    .copied()
                    .filter(|node| is_descendant_or_self(scene, *node, root)),
            );
        }
    }

    fn index_root_plan_fragments(&mut self, scene: &RetainedScene) {
        let Some(root_owner_ops) = Arc::make_mut(&mut self.canvas)
            .compiled_plan
            .as_ref()
            .map(|plan| plan.root_owner_op_indices())
        else {
            return;
        };
        let layers = scene.nodes[&scene.root]
            .content
            .order
            .values()
            .copied()
            .filter(|id| matches!(scene.nodes[id].kind, NodeKind::Layer(_)))
            .collect::<Vec<_>>();
        for id in layers {
            let Some(&location) = self.layer_command_locations.get(&id) else {
                continue;
            };
            let command = Arc::make_mut(&mut self.canvas).command_lists[0].commands
                [location.command_index]
                .clone();
            let temporary = self.push_command_list();
            Arc::make_mut(&mut self.canvas).command_lists[temporary]
                .commands
                .push(command);
            let fragment = Arc::make_mut(&mut self.canvas).compile(temporary);
            let _ = Arc::make_mut(&mut self.canvas).command_lists.pop();
            let Some(ops) = Arc::make_mut(&mut self.canvas)
                .compiled_plan
                .as_ref()
                .and_then(|plan| plan.root_fragment_range(&fragment, id, &root_owner_ops))
            else {
                continue;
            };
            let mut nodes = Vec::new();
            scene.collect_subtree(id, &mut nodes);
            self.root_plan_fragments.insert(
                id,
                RootPlanFragment {
                    ops,
                    // Initial command lists can be interleaved with later root siblings. They
                    // remain stable tombstones when this fragment is removed; appended fragments
                    // still record reclaimable tail ranges.
                    command_lists: 0..0,
                    nodes: nodes.into_iter().collect(),
                    reassigned_batches: HashMap::default(),
                    removed_batch: None,
                },
            );
        }
    }

    fn rebuild_painter_metadata(&mut self, scene: &RetainedScene) {
        let old_keys = Arc::make_mut(&mut self.canvas).painter_keys.clone();
        self.painter_bases.clear();
        let mut leaves = Vec::new();
        collect_scene_leaves(scene, scene.root, &mut leaves);
        let draw_capacity = self.arenas.draws.values().len();
        let mut keys = vec![PainterKey::inactive(); draw_capacity];
        let mut batches = vec![u32::MAX; draw_capacity];
        let mut batch_counts = Vec::new();
        for &id in &leaves {
            let parent = scene.nodes[&id].parent.expect("leaf has retained parent");
            let base = painter_path(scene, id);
            self.painter_bases.insert(id, base.clone());
            self.painter_parents.insert(id, parent);
            self.write_node_painter_metadata(
                id,
                base,
                None,
                &mut keys,
                &mut batches,
                &mut batch_counts,
            );
        }
        let plan = Arc::new(
            Arc::make_mut(&mut self.canvas).compile(crate::shared::execution::ROOT_COMMAND_LIST_ID),
        );
        batches = (*plan.draw_batch_ids).clone();
        Arc::make_mut(&mut self.canvas).compiled_plan = Some(plan.clone());
        batches.resize(draw_capacity, u32::MAX);
        self.node_batches.clear();
        for id in leaves {
            let physical = self.node_physical_draws(id);
            let mut ids = physical
                .iter()
                .filter_map(|&draw| (batches[draw] != u32::MAX).then_some(batches[draw]));
            if let Some(batch) = ids.next()
                && ids.all(|candidate| candidate == batch)
            {
                self.node_batches.insert(id, batch);
            }
        }
        self.container_batches.clear();
        collect_container_batches(
            scene,
            scene.root,
            &self.node_batches,
            &mut self.container_batches,
        );
        self.container_batches
            .extend(plan.retained_batch_ids.iter().map(|(owner, &batch)| {
                (
                    RetainedParent {
                        node: owner.node,
                        branch: match owner.branch {
                            RetainedBatchBranch::Content => RetainedChildBranch::Content,
                            RetainedBatchBranch::Mask => RetainedChildBranch::Mask,
                        },
                    },
                    batch,
                )
            }));
        self.flat_plan_has_draws = exec_ops_have_batches(&plan.ops);
        let canvas = Arc::make_mut(&mut self.canvas);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter = changed_value_ranges(old_keys.as_deref().unwrap_or(&[]), &keys);
        }
        canvas.painter_keys = Some(keys);
        canvas.stable_batch_counts = Some(count_stable_batches(&batches));
        canvas.stable_batch_ids = Some(batches);
    }

    fn update_painter_metadata(&mut self, changed: &HashSet<RetainedNodeId>) {
        let draw_capacity = self.arenas.draws.values().len();
        let canvas = Arc::make_mut(&mut self.canvas);
        let mut keys = canvas
            .painter_keys
            .take()
            .unwrap_or_else(|| vec![PainterKey::inactive(); draw_capacity]);
        let mut batches = canvas
            .stable_batch_ids
            .take()
            .unwrap_or_else(|| vec![u32::MAX; draw_capacity]);
        let mut batch_counts = canvas
            .stable_batch_counts
            .take()
            .unwrap_or_else(|| count_stable_batches(&batches));
        keys.resize(draw_capacity, PainterKey::inactive());
        batches.resize(draw_capacity, u32::MAX);
        let mut dirty = Vec::new();
        if let Some(changes) = &canvas.buffer_changes {
            // One journal commit can patch plan membership and then apply topology cleanup.
            // Preserve earlier painter writes so the GPU never observes only the last sub-step.
            dirty.extend(changes.painter.iter().cloned());
            for range in &changes.draws {
                keys[range.clone()].fill(PainterKey::inactive());
                for physical in range.clone() {
                    set_stable_batch(&mut batches, &mut batch_counts, physical, u32::MAX);
                }
                dirty.push(range.clone());
            }
        }
        canvas.painter_keys = Some(keys);
        canvas.stable_batch_ids = Some(batches);
        canvas.stable_batch_counts = Some(batch_counts);

        for &id in changed {
            let Some(base) = self.painter_bases.get(&id).cloned() else {
                continue;
            };
            let (mut keys, mut batches, mut batch_counts) = {
                let canvas = Arc::make_mut(&mut self.canvas);
                (
                    canvas.painter_keys.take().unwrap(),
                    canvas.stable_batch_ids.take().unwrap(),
                    canvas.stable_batch_counts.take().unwrap(),
                )
            };
            let batch = self.node_batches.get(&id).copied();
            if self.write_node_painter_metadata(
                id,
                base,
                batch,
                &mut keys,
                &mut batches,
                &mut batch_counts,
            ) {
                dirty.push(self.arenas.draws.range(self.chunks[&id].draws));
            }
            let canvas = Arc::make_mut(&mut self.canvas);
            canvas.painter_keys = Some(keys);
            canvas.stable_batch_ids = Some(batches);
            canvas.stable_batch_counts = Some(batch_counts);
        }
        let canvas = Arc::make_mut(&mut self.canvas);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter = merge_index_ranges(dirty);
        }
    }

    fn refresh_compiled_plan(&mut self) {
        let canvas = Arc::make_mut(&mut self.canvas);
        if canvas.compiled_plan.is_some() {
            return;
        }
        canvas.compiled_plan = Some(Arc::new(
            canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID),
        ));
    }

    /// Synchronizes stable GPU batch membership after a full execution-plan rebuild.
    ///
    /// A non-plain retained leaf can own draws on both sides of an offscreen layer, so it has no
    /// single `node_batches` entry. Clearing that leaf's dirty draw slots before recompiling used
    /// to leave them inactive even though the rebuilt plan still referenced them. Deriving the
    /// table from the new plan is the authoritative path whenever plan structure is rebuilt and
    /// also covers same-length content mutations that renumber later batches.
    fn sync_stable_batches_from_plan(&mut self) {
        let canvas = Arc::make_mut(&mut self.canvas);
        let Some(plan) = canvas.compiled_plan.as_ref() else {
            return;
        };
        let mut batches = (*plan.draw_batch_ids).clone();
        batches.resize(self.arenas.draws.values().len(), u32::MAX);
        let dirty =
            changed_value_ranges(canvas.stable_batch_ids.as_deref().unwrap_or(&[]), &batches);
        if let Some(changes) = &mut canvas.buffer_changes {
            changes.painter.extend(dirty);
            changes.painter = merge_index_ranges(std::mem::take(&mut changes.painter));
        }
        canvas.stable_batch_counts = Some(count_stable_batches(&batches));
        canvas.stable_batch_ids = Some(batches);
    }

    fn write_node_painter_metadata(
        &self,
        id: RetainedNodeId,
        base: Arc<[u128]>,
        batch: Option<u32>,
        keys: &mut [PainterKey],
        batches: &mut [u32],
        batch_counts: &mut Vec<u32>,
    ) -> bool {
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        let plan = chunk
            .canvas
            .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut changed = false;
        for (local_order, local_draw) in plan.draw_order.iter().copied().enumerate() {
            let physical = draw_base + local_draw as usize;
            let key = PainterKey {
                path: base.clone(),
                local: local_order as u32,
            };
            changed |= keys[physical] != key;
            keys[physical] = key;
            if let Some(batch) = batch {
                changed |= set_stable_batch(batches, batch_counts, physical, batch);
            }
        }
        changed
    }

    fn node_physical_draws(&self, id: RetainedNodeId) -> Vec<usize> {
        let chunk = &self.chunks[&id];
        let draw_base = self.arenas.draws.range(chunk.draws).start;
        chunk
            .canvas
            .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID)
            .draw_order
            .iter()
            .copied()
            .map(|draw| draw_base + draw as usize)
            .collect()
    }

    fn influenced_bounds(
        &self,
        scene: &RetainedScene,
        mut id: RetainedNodeId,
        mut bounds: Bounds,
    ) -> Bounds {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        while let Some(parent) = scene.nodes[&id].parent {
            let parent_node = &scene.nodes[&parent.node];
            if let NodeKind::Layer(layer) = &parent_node.kind {
                bounds = match layer {
                    RetainedLayerDescriptor::ClipPath { .. }
                    | RetainedLayerDescriptor::ClipSdf(_)
                    | RetainedLayerDescriptor::Isolate { .. }
                    | RetainedLayerDescriptor::Opacity { .. }
                    | RetainedLayerDescriptor::Blend { .. } => self.chunks[&parent.node]
                        .canvas
                        .draw_records
                        .first()
                        .map_or(bounds, |draw| {
                            let clip = draw.pixel_bounds;
                            bounds.intersect(Bounds::new(clip.x0, clip.y0, clip.x1, clip.y1))
                        }),
                    RetainedLayerDescriptor::Filter {
                        filter: value,
                        sample_region,
                    } => {
                        let dependency = filter::region_bounds(sample_region)
                            .outset(filter::filter_dependency_outset(value));
                        let changed = bounds.intersect(dependency);
                        if changed.is_empty() {
                            Bounds::new(0, 0, 0, 0)
                        } else {
                            changed
                                .outset(filter::filter_outset(value))
                                .intersect(filter::unclipped_filtered_region_bounds(
                                    value,
                                    sample_region,
                                ))
                                .intersect(canvas_bounds)
                        }
                    }
                    RetainedLayerDescriptor::Mask(mask) => {
                        bounds.intersect(filter::region_bounds(&mask.region))
                    }
                    RetainedLayerDescriptor::Backdrop { .. } => bounds,
                };
            }
            id = parent.node;
        }
        bounds
    }

    /// Resolves backdrop dependencies from the persistent hierarchy and painter index. Frame
    /// node bounds already include ordinary filter influence, while a backdrop additionally
    /// depends on earlier siblings intersecting its sample region.
    fn incremental_backdrop_damage(
        &self,
        scene: &RetainedScene,
        sources: &[(Option<RetainedNodeId>, Bounds)],
    ) -> (Vec<(RetainedNodeId, Bounds)>, Vec<RetainedNodeId>) {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        let mut dirty = Vec::new();
        let mut pending = sources.to_vec();
        let mut backdrops = self
            .nonlocal_dependencies
            .iter()
            .copied()
            .collect::<Vec<_>>();
        backdrops.sort_unstable_by_key(|id| painter_path(scene, *id));
        for backdrop in backdrops {
            let Some(chunk) = self.chunks.get(&backdrop) else {
                continue;
            };
            let backdrop_painter = painter_path(scene, backdrop);
            let mut affected_output = Bounds::new(0, 0, 0, 0);
            for dependency in &chunk.backdrop_dependencies {
                for &(source, bounds) in &pending {
                    let affected = if source == Some(backdrop) {
                        self.node_bounds
                            .get(&backdrop)
                            .copied()
                            .unwrap_or(dependency.output)
                    } else if let Some(source) = source {
                        let Some(source_painter) =
                            self.painter_bases.get(&source).cloned().or_else(|| {
                                scene
                                    .nodes
                                    .contains_key(&source)
                                    .then(|| painter_path(scene, source))
                            })
                        else {
                            continue;
                        };
                        if source_painter.as_ref() >= backdrop_painter.as_ref() {
                            continue;
                        }
                        let sampled = bounds.intersect(dependency.dependency);
                        if sampled.is_empty() {
                            continue;
                        }
                        sampled
                            .outset(dependency.output_outset)
                            .intersect(dependency.output)
                    } else {
                        // Manual invalidation is already expressed in root output coordinates but
                        // has no painter owner. Any intersecting backdrop may sample those pixels.
                        let sampled = bounds.intersect(dependency.dependency);
                        if sampled.is_empty() {
                            continue;
                        }
                        sampled
                            .outset(dependency.output_outset)
                            .intersect(dependency.output)
                    };
                    affected_output = affected_output.union(affected);
                }
            }
            if !affected_output.is_empty() {
                dirty.push(backdrop);
                // A changed earlier backdrop becomes sampled background for later backdrops.
                pending.push((Some(backdrop), affected_output));
                damage
                    .entry(backdrop)
                    .and_modify(|current| *current = current.union(affected_output))
                    .or_insert(affected_output);
            }
        }
        (damage.into_iter().collect(), dirty)
    }

    fn patch_frame_override(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        changed: &HashSet<RetainedNodeId>,
    ) {
        let Some(mut frame) = previous else {
            self.rebuild_frame_override(scene);
            return;
        };
        let mut patches = Vec::with_capacity(changed.len());
        for &id in changed {
            let Some(old) = frame.node_state(id) else {
                self.rebuild_frame_override(scene);
                return;
            };
            let node = &scene.nodes[&id];
            let Some(chunk) = self.chunks.get(&id) else {
                self.rebuild_frame_override(scene);
                return;
            };
            let raw_bounds = chunk.canvas.visual_bounds();
            let new = crate::canvas::RetainedNodeState {
                revision: SceneRevision::new(node.generation),
                // Position-only patches in scenes with layers still need the new influenced
                // bounds. Keeping `old.bounds` forced later frames to rebuild the full retained
                // frame/spatial index and missed the moved node's new backdrop dependencies.
                bounds: self.influenced_bounds(scene, id, raw_bounds),
                kind: match node.kind {
                    NodeKind::Layer(_) => RetainedNodeKind::Layer,
                    NodeKind::Scene { .. } => RetainedNodeKind::Scene,
                    NodeKind::Group => {
                        self.rebuild_frame_override(scene);
                        return;
                    }
                },
                placement_bits: None,
                ..old
            };
            patches.push(RetainedNodePatch {
                old: Some(old),
                new: Some(new),
            });
            self.raw_node_bounds.insert(id, raw_bounds);
        }
        let index = retained_patch_index(&patches);
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        if depth > 255 {
            self.rebuild_frame_override(scene);
            self.rebuild_spatial_index();
            return;
        }
        let (backdrop_damage, dirty_backdrops) = if self.nonlocal_dependencies.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let source_damage = patches
                .iter()
                .map(|patch| {
                    let node = patch.new.or(patch.old).unwrap();
                    let bounds = match (patch.old, patch.new) {
                        (Some(old), Some(new)) => old.bounds.union(new.bounds),
                        (Some(old), None) => old.bounds,
                        (None, Some(new)) => new.bounds,
                        (None, None) => unreachable!("retained patch has a node"),
                    };
                    (Some(node.id), bounds)
                })
                .chain(
                    self.canvas
                        .invalidated_bounds
                        .iter()
                        .copied()
                        .map(|bounds| (None, bounds)),
                )
                .collect::<Vec<_>>();
            self.incremental_backdrop_damage(scene, &source_damage)
        };
        for patch in &patches {
            if patch.old.map(|node| node.bounds) != patch.new.map(|node| node.bounds) {
                self.set_node_bounds(patch.new.unwrap().id, patch.new.map(|node| node.bounds));
            }
        }
        frame.version = Some(scene.version.get());
        frame.delta = Some(Arc::new(RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: patches.into(),
            previous,
            depth,
            damage: backdrop_damage.into(),
            dirty_backdrops: dirty_backdrops.into(),
            backdrop_damage_complete: true,
            index: Arc::new(index),
        }));
        frame.invalidated_bounds = Arc::make_mut(&mut self.canvas).invalidated_bounds.clone();
        frame.invalidate_all = Arc::make_mut(&mut self.canvas).invalidate_all;
        frame.dependency_free = self.dependency_free;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        Arc::make_mut(&mut self.canvas).retained_frame_override = Some(frame);
    }

    fn patch_topology_frame_override(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        changes: &SceneChangeSet,
        damage: &[(RetainedNodeId, Bounds)],
    ) -> bool {
        let Some(mut frame) = previous else {
            return false;
        };
        let mut patches = Vec::new();
        for &id in &changes.removed_nodes {
            if let Some(old) = frame.node_state(id) {
                patches.push(RetainedNodePatch {
                    old: Some(old),
                    new: None,
                });
            }
        }
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            let chunk = &self.chunks[&id];
            let old = frame.node_state(id);
            let (bounds, kind) = match &node.kind {
                NodeKind::Scene { .. } => (
                    self.influenced_bounds(scene, id, chunk.canvas.visual_bounds()),
                    RetainedNodeKind::Scene,
                ),
                NodeKind::Layer(_) => {
                    let mut bounds =
                        self.influenced_bounds(scene, id, chunk_layer_influence_bounds(chunk));
                    let mut leaves = Vec::new();
                    collect_scene_leaves(scene, id, &mut leaves);
                    for leaf_id in leaves {
                        let leaf = &self.chunks[&leaf_id];
                        bounds = bounds.union(self.influenced_bounds(
                            scene,
                            leaf_id,
                            leaf.canvas.visual_bounds(),
                        ));
                    }
                    (bounds, RetainedNodeKind::Layer)
                }
                NodeKind::Group => continue,
            };
            patches.push(RetainedNodePatch {
                old,
                new: Some(crate::canvas::RetainedNodeState {
                    id,
                    revision: SceneRevision::new(node.generation),
                    bounds,
                    order: old.map_or(0, |node| node.order),
                    kind,
                    placement_bits: None,
                }),
            });
        }
        let mut explicit_damage = damage.to_vec();
        explicit_damage.extend(patches.iter().map(|patch| {
            let node = patch.new.or(patch.old).unwrap();
            let bounds = match (patch.old, patch.new) {
                (Some(old), Some(new)) => old.bounds.union(new.bounds),
                (Some(old), None) => old.bounds,
                (None, Some(new)) => new.bounds,
                (None, None) => unreachable!("retained patch has at least one state"),
            };
            (node.id, bounds)
        }));
        let index = retained_patch_index(&patches);
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        if depth > 255 {
            return false;
        }
        for patch in &patches {
            let id = patch.new.or(patch.old).unwrap().id;
            if patch.old.map(|node| node.bounds) != patch.new.map(|node| node.bounds) {
                self.set_node_bounds(id, patch.new.map(|node| node.bounds));
            }
            if let Some(chunk) = self.chunks.get(&id) {
                self.raw_node_bounds
                    .insert(id, chunk.canvas.visual_bounds());
            } else {
                self.raw_node_bounds.remove(&id);
            }
        }
        frame.version = Some(scene.version.get());
        frame.delta = Some(Arc::new(RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: patches.into(),
            previous,
            depth,
            damage: explicit_damage.into(),
            dirty_backdrops: Arc::new([]),
            backdrop_damage_complete: false,
            index: Arc::new(index),
        }));
        let canvas = Arc::make_mut(&mut self.canvas);
        frame.invalidated_bounds = canvas.invalidated_bounds.clone();
        frame.invalidate_all = canvas.invalidate_all;
        frame.dependency_free = self.dependency_free;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        canvas.retained_frame_override = Some(frame);
        true
    }

    fn rebuild_frame_override(&mut self, scene: &RetainedScene) {
        let canvas = Arc::make_mut(&mut self.canvas);
        canvas.retained_frame_override = None;
        let mut frame = canvas
            .collect_retained_frame()
            .expect("persistent materialized scene has retained identity");
        frame.version = Some(scene.version.get());
        frame.dependency_free = self.dependency_free;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        canvas.retained_frame_override = Some(frame);
    }

    fn rebuild_spatial_index(&mut self) {
        self.node_bounds.clear();
        self.raw_node_bounds.clear();
        let tile_count = Arc::make_mut(&mut self.canvas).width_in_tiles() as usize
            * Arc::make_mut(&mut self.canvas).height_in_tiles() as usize;
        self.node_tiles.clear();
        self.node_tiles.resize(tile_count, HashSet::default());
        let frame = Arc::make_mut(&mut self.canvas)
            .retained_frame_override
            .clone()
            .expect("persistent frame override");
        for &node in frame.nodes.iter() {
            self.set_node_bounds(node.id, Some(node.bounds));
        }
        for (&id, chunk) in &self.chunks {
            self.raw_node_bounds
                .insert(id, chunk.canvas.visual_bounds());
        }
    }

    fn set_node_bounds(&mut self, id: RetainedNodeId, bounds: Option<Bounds>) {
        let bounds = bounds.filter(|bounds| !bounds.is_empty());
        let old = match self.node_bounds.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = *entry.get();
                if Some(old) == bounds {
                    return;
                }
                if let Some(bounds) = bounds {
                    *entry.get_mut() = bounds;
                } else {
                    entry.remove();
                }
                Some(old)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                if let Some(bounds) = bounds {
                    entry.insert(bounds);
                }
                None
            }
        };
        if let Some(old) = old {
            for tile in self.tiles_for_bounds(old) {
                self.node_tiles[tile].remove(&id);
            }
        }
        if let Some(bounds) = bounds {
            for tile in self.tiles_for_bounds(bounds) {
                self.node_tiles[tile].insert(id);
            }
        }
    }

    fn tiles_for_bounds(&self, bounds: Bounds) -> Vec<usize> {
        let canvas = &self.canvas;
        let width = canvas.width_in_tiles();
        let height = canvas.height_in_tiles();
        let x0 = bounds.x0.max(0) as u32 / crate::TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / crate::TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(crate::TILE_SIZE)
            .min(width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(crate::TILE_SIZE)
            .min(height);
        let mut tiles = Vec::new();
        for y in y0.min(height)..y1 {
            for x in x0.min(width)..x1 {
                tiles.push((y * width + x) as usize);
            }
        }
        tiles
    }

    fn spatial_candidates(&self, bounds: Bounds) -> HashSet<RetainedNodeId> {
        let mut candidates = HashSet::default();
        for tile in self.tiles_for_bounds(bounds) {
            candidates.extend(self.node_tiles[tile].iter().copied());
        }
        candidates
    }

    fn root_reorder_damage(
        &self,
        old_bases: &HashMap<RetainedNodeId, Arc<[u128]>>,
        changed: &HashSet<RetainedNodeId>,
    ) -> Vec<(RetainedNodeId, Bounds)> {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        for &id in changed {
            let (Some(&bounds), Some(old), Some(new)) = (
                self.node_bounds.get(&id),
                old_bases.get(&id),
                self.painter_bases.get(&id),
            ) else {
                continue;
            };
            for candidate in self.spatial_candidates(bounds) {
                if candidate == id || !self.node_bounds.contains_key(&candidate) {
                    continue;
                }
                let Some(candidate_new) = self.painter_bases.get(&candidate) else {
                    continue;
                };
                let candidate_old = old_bases.get(&candidate).unwrap_or(candidate_new);
                if (old.as_ref() < candidate_old.as_ref())
                    == (new.as_ref() < candidate_new.as_ref())
                {
                    continue;
                }
                let overlap = bounds.intersect(self.node_bounds[&candidate]);
                if !overlap.is_empty() {
                    damage
                        .entry(id)
                        .and_modify(|current| *current = current.union(overlap))
                        .or_insert(overlap);
                }
            }
        }
        damage.into_iter().collect()
    }

    fn push_command_list(&mut self) -> usize {
        let canvas = Arc::make_mut(&mut self.canvas);
        let id = canvas.command_lists.len();
        canvas.command_lists.push(CommandList::default());
        id
    }
}

fn sync_arena<T: Copy>(
    dst: &mut Vec<T>,
    arena: &mut SceneArena<T>,
    vacant: T,
) -> Vec<std::ops::Range<usize>> {
    dst.resize(arena.values().len(), vacant);
    let ranges = arena.take_dirty_ranges();
    for range in &ranges {
        dst[range.clone()].copy_from_slice(&arena.values()[range.clone()]);
    }
    dst.truncate(arena.values().len());
    ranges
}

fn range_bytes<T>(ranges: &[std::ops::Range<usize>]) -> u64 {
    ranges
        .iter()
        .map(|range| (range.len() * std::mem::size_of::<T>()) as u64)
        .sum()
}

fn changed_value_ranges<T: PartialEq>(old: &[T], new: &[T]) -> Vec<std::ops::Range<usize>> {
    let len = old.len().max(new.len());
    let mut ranges = Vec::new();
    let mut start = None;
    for index in 0..len {
        let changed = old.get(index) != new.get(index);
        match (start, changed) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                ranges.push(begin..index);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        ranges.push(begin..len);
    }
    ranges
}

fn merge_index_ranges(mut ranges: Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
    ranges.retain(|range| !range.is_empty());
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged = Vec::<std::ops::Range<usize>>::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn retained_patch_index(patches: &[RetainedNodePatch]) -> HashMap<RetainedNodeId, usize> {
    patches
        .iter()
        .enumerate()
        .map(|(index, patch)| (patch.new.or(patch.old).unwrap().id, index))
        .collect()
}

fn prune_shadowed_delta(
    mut previous: Option<Arc<RetainedFrameDelta>>,
    index: &HashMap<RetainedNodeId, usize>,
) -> (Option<Arc<RetainedFrameDelta>>, u16) {
    while previous
        .as_ref()
        .is_some_and(|delta| delta.index.keys().all(|id| index.contains_key(id)))
    {
        previous = previous.unwrap().previous.clone();
    }
    let depth = previous.as_ref().map_or(1, |delta| delta.depth + 1);
    (previous, depth)
}

fn collect_scene_leaves(
    scene: &RetainedScene,
    parent: RetainedNodeId,
    leaves: &mut Vec<RetainedNodeId>,
) {
    let node = &scene.nodes[&parent];
    for &child in node.content.values().chain(node.mask.values()) {
        match scene.nodes[&child].kind {
            NodeKind::Scene { .. } => leaves.push(child),
            NodeKind::Group | NodeKind::Layer(_) => collect_scene_leaves(scene, child, leaves),
        }
    }
}

fn count_stable_batches(batches: &[u32]) -> Vec<u32> {
    let Some(maximum) = batches
        .iter()
        .copied()
        .filter(|&batch| batch != u32::MAX)
        .max()
    else {
        return Vec::new();
    };
    let mut counts = vec![0; maximum as usize + 1];
    for &batch in batches {
        if batch != u32::MAX {
            counts[batch as usize] += 1;
        }
    }
    counts
}

fn exec_ops_have_batches(ops: &[crate::shared::execution::ExecOp]) -> bool {
    ops.iter().any(|op| match op {
        crate::shared::execution::ExecOp::DrawBatch { .. } => true,
        crate::shared::execution::ExecOp::OffscreenLayer { children, .. } => {
            exec_ops_have_batches(children)
        }
        crate::shared::execution::ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            exec_ops_have_batches(content) || exec_ops_have_batches(mask)
        }
        _ => false,
    })
}

fn set_stable_batch(
    batches: &mut [u32],
    counts: &mut Vec<u32>,
    physical: usize,
    batch: u32,
) -> bool {
    let previous = batches[physical];
    if previous == batch {
        return false;
    }
    if previous != u32::MAX {
        counts[previous as usize] -= 1;
    }
    if batch != u32::MAX {
        counts.resize(counts.len().max(batch as usize + 1), 0);
        counts[batch as usize] += 1;
    }
    batches[physical] = batch;
    true
}

fn collect_container_batches(
    scene: &RetainedScene,
    id: RetainedNodeId,
    leaf_batches: &HashMap<RetainedNodeId, u32>,
    containers: &mut HashMap<RetainedParent, u32>,
) -> Option<u32> {
    let node = &scene.nodes[&id];
    let content =
        collect_branch_batch(scene, RetainedParent::content(id), leaf_batches, containers);
    if let Some(batch) = content {
        containers.insert(RetainedParent::content(id), batch);
    }
    if matches!(node.kind, NodeKind::Layer(RetainedLayerDescriptor::Mask(_))) {
        let mask = collect_branch_batch(scene, RetainedParent::mask(id), leaf_batches, containers);
        if let Some(batch) = mask {
            containers.insert(RetainedParent::mask(id), batch);
        }
    }
    matches!(node.kind, NodeKind::Group)
        .then_some(content)
        .flatten()
}

fn collect_branch_batch(
    scene: &RetainedScene,
    parent: RetainedParent,
    leaf_batches: &HashMap<RetainedNodeId, u32>,
    containers: &mut HashMap<RetainedParent, u32>,
) -> Option<u32> {
    let mut batch = None;
    for &child in scene.nodes[&parent.node]
        .children(parent.branch)
        .expect("validated retained branch")
        .values()
    {
        let candidate = match scene.nodes[&child].kind {
            NodeKind::Scene { .. } => leaf_batches.get(&child).copied(),
            NodeKind::Group => collect_container_batches(scene, child, leaf_batches, containers),
            NodeKind::Layer(_) => {
                collect_container_batches(scene, child, leaf_batches, containers);
                None
            }
        };
        let Some(candidate) = candidate else {
            continue;
        };
        if batch.is_some_and(|batch| batch != candidate) {
            return None;
        }
        batch = Some(candidate);
    }
    batch
}

fn painter_path(scene: &RetainedScene, mut id: RetainedNodeId) -> Arc<[u128]> {
    let mut reversed = Vec::new();
    while id != scene.root {
        let parent = scene.nodes[&id]
            .parent
            .expect("non-root retained node has parent");
        let children = scene.nodes[&parent.node]
            .children(parent.branch)
            .expect("validated retained branch");
        reversed.push((
            parent.branch,
            children
                .key_of(id)
                .expect("child exists in retained parent"),
        ));
        id = parent.node;
    }
    reversed.reverse();
    reversed
        .into_iter()
        .flat_map(|(branch, key)| [branch as u128, key])
        .collect::<Vec<_>>()
        .into()
}

fn is_descendant_or_self(
    scene: &RetainedScene,
    mut id: RetainedNodeId,
    ancestor: RetainedNodeId,
) -> bool {
    loop {
        if id == ancestor {
            return true;
        }
        let Some(parent) = scene.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        id = parent.node;
    }
}

fn first_command_list(command: &Command) -> usize {
    match command {
        Command::MaterializedRetainedScene { children, .. } | Command::Layer { children, .. } => {
            *children
        }
        Command::MaskLayer { content, mask, .. } => (*content).min(*mask),
        Command::Draw(_) | Command::RetainedScene { .. } => {
            unreachable!("retained root layer command owns at least one child command list")
        }
    }
}

fn inactive_draw() -> DrawRecord {
    DrawRecord {
        path_id: DrawRecord::NONE,
        glyph_run_id: DrawRecord::NONE,
        sdf_offset: DrawRecord::NONE,
        sdf_len: 0,
        sdf_shadow_offset: DrawRecord::NONE,
        sdf_shadow_len: 0,
        brush_offset: DrawRecord::NONE,
        brush_len: 0,
        tag: DrawTagWord::default(),
        fill_rule: FillRuleWord::default(),
        pixel_bounds: Default::default(),
        solid_rect: 0,
    }
}

fn is_plain_fragment(canvas: &Canvas) -> bool {
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    plan.layer_stack_data.is_empty()
        && plan
            .ops
            .iter()
            .all(|op| matches!(op, crate::shared::execution::ExecOp::DrawBatch { .. }))
}

fn scene_node_placement(node: &SceneNode) -> (Option<Arc<Canvas>>, Option<(u64, u64)>) {
    match &node.kind {
        NodeKind::Scene { canvas, position } => (
            Some(canvas.clone()),
            Some((position.x.to_bits(), position.y.to_bits())),
        ),
        _ => (None, None),
    }
}

fn command_has_filter_resources(command: &Command) -> bool {
    matches!(
        command,
        Command::Layer {
            layer: Layer::Filter { .. } | Layer::Backdrop { .. },
            ..
        }
    )
}

fn backdrop_dependencies(canvas: &Canvas) -> Vec<BackdropDependency> {
    let canvas_bounds = Bounds::canvas(canvas.physical_width(), canvas.physical_height());
    canvas
        .command_lists
        .iter()
        .flat_map(|list| &list.commands)
        .filter_map(|command| {
            let Command::Layer {
                layer:
                    Layer::Backdrop {
                        filter: value,
                        sample_region,
                    },
                ..
            } = command
            else {
                return None;
            };
            Some(BackdropDependency {
                dependency: filter::region_bounds(sample_region)
                    .outset(filter::filter_dependency_outset(value)),
                output: filter::unclipped_filtered_region_bounds(value, sample_region)
                    .intersect(canvas_bounds),
                output_outset: filter::filter_outset(value),
            })
        })
        .collect()
}

fn chunk_layer_influence_bounds(chunk: &SceneChunk) -> Bounds {
    let command = &chunk.canvas.command_lists[chunk.canvas.root_commands].commands[0];
    match command {
        Command::Layer {
            draw,
            layer:
                crate::shared::layer::Layer::Clip
                | crate::shared::layer::Layer::ClipSdf { .. }
                | crate::shared::layer::Layer::Isolate
                | crate::shared::layer::Layer::Opacity(_)
                | crate::shared::layer::Layer::Blend(_),
            ..
        } => {
            let bounds = chunk.canvas.draw_records[*draw].pixel_bounds;
            Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
        }
        Command::Layer {
            layer:
                crate::shared::layer::Layer::Filter {
                    filter: value,
                    sample_region,
                }
                | crate::shared::layer::Layer::Backdrop {
                    filter: value,
                    sample_region,
                },
            ..
        } => filter::unclipped_filtered_region_bounds(value, sample_region),
        Command::MaskLayer { layer, .. } => filter::region_bounds(&layer.region),
        _ => unreachable!("retained layer chunk has one root layer command"),
    }
}

#[derive(Clone)]
struct JournalEntry {
    version: SceneVersion,
    changes: SceneChangeSet,
}

pub struct RetainedScene {
    id: u64,
    width: u32,
    height: u32,
    scale: f32,
    root: RetainedNodeId,
    version: SceneVersion,
    nodes: HashMap<RetainedNodeId, SceneNode>,
    journal: VecDeque<JournalEntry>,
}

impl RetainedScene {
    pub fn new(
        width: u32,
        height: u32,
        scale: f32,
        root: RetainedNodeId,
    ) -> Result<Self, RetainedSceneError> {
        validate_size(width, height, scale)?;
        let mut nodes = HashMap::default();
        nodes.insert(root, SceneNode::group(None));
        Ok(Self {
            id: NEXT_SCENE_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            scale,
            root,
            version: SceneVersion::INITIAL,
            nodes,
            journal: VecDeque::new(),
        })
    }

    pub fn version(&self) -> SceneVersion {
        self.version
    }

    pub fn root(&self) -> RetainedNodeId {
        self.root
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (
            ((self.width as f64) * f64::from(self.scale)).ceil() as u32,
            ((self.height as f64) * f64::from(self.scale)).ceil() as u32,
        )
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub fn transaction(&mut self) -> RetainedSceneTransaction<'_> {
        RetainedSceneTransaction {
            scene: self,
            mutations: Vec::new(),
        }
    }

    pub(crate) fn changes_since(&self, version: SceneVersion) -> Option<SceneChangeSet> {
        if version == self.version {
            return Some(SceneChangeSet::default());
        }
        let first = self.journal.front()?.version.get();
        if version.get().saturating_add(1) < first {
            return None;
        }
        let mut changes = SceneChangeSet::default();
        for entry in self.journal.iter().filter(|entry| entry.version > version) {
            changes.merge(&entry.changes);
        }
        Some(changes)
    }

    pub(crate) fn to_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new_retained(self.width, self.height, self.scale, self.root);
        self.append_children(&mut canvas, RetainedParent::content(self.root));
        canvas
    }

    fn append_children(&self, canvas: &mut Canvas, parent: RetainedParent) {
        let node = &self.nodes[&parent.node];
        let children = node
            .children(parent.branch)
            .expect("validated scene branch");
        for child in children.values() {
            self.append_node(canvas, *child);
        }
    }

    fn append_node(&self, canvas: &mut Canvas, id: RetainedNodeId) {
        let node = &self.nodes[&id];
        match &node.kind {
            NodeKind::Group => self.append_children(canvas, RetainedParent::content(id)),
            NodeKind::Scene {
                canvas: child,
                position,
            } => canvas.append_retained_scene(
                id,
                SceneRevision::new(node.generation),
                child.clone(),
                *position,
            ),
            NodeKind::Layer(layer) => {
                let key = RetainedLayerKey::new(id, SceneRevision::new(node.generation));
                match layer {
                    RetainedLayerDescriptor::ClipPath {
                        path,
                        transform,
                        rule,
                        tolerance,
                    } => canvas.push_retained_clip_layer(
                        key,
                        path.clone(),
                        *transform,
                        *rule,
                        *tolerance,
                    ),
                    RetainedLayerDescriptor::ClipSdf(sdf) => {
                        canvas.push_retained_clip_sdf_layer(key, *sdf)
                    }
                    RetainedLayerDescriptor::Isolate {
                        path,
                        transform,
                        tolerance,
                    } => canvas.push_retained_isolate_layer(
                        key,
                        path.clone(),
                        *transform,
                        *tolerance,
                    ),
                    RetainedLayerDescriptor::Opacity {
                        path,
                        transform,
                        tolerance,
                        opacity,
                    } => canvas.push_retained_opacity_layer(
                        key,
                        path.clone(),
                        *transform,
                        *tolerance,
                        *opacity,
                    ),
                    RetainedLayerDescriptor::Blend {
                        path,
                        transform,
                        tolerance,
                        mix,
                        compose,
                    } => canvas.push_retained_blend_layer(
                        key,
                        path.clone(),
                        *transform,
                        *tolerance,
                        *mix,
                        *compose,
                    ),
                    RetainedLayerDescriptor::Filter {
                        filter,
                        sample_region,
                    } => canvas.push_retained_filter_layer(
                        key,
                        filter.clone(),
                        sample_region.clone(),
                    ),
                    RetainedLayerDescriptor::Backdrop {
                        filter,
                        sample_region,
                    } => canvas.push_retained_backdrop_layer(
                        key,
                        filter.clone(),
                        sample_region.clone(),
                    ),
                    RetainedLayerDescriptor::Mask(mask) => {
                        let mut mask_canvas =
                            Canvas::new_retained(self.width, self.height, self.scale, id);
                        self.append_children(&mut mask_canvas, RetainedParent::mask(id));
                        canvas.push_retained_mask_layer(key, mask_canvas, mask.clone());
                    }
                }
                self.append_children(canvas, RetainedParent::content(id));
                canvas.pop_layer();
            }
        }
    }

    fn commit_mutations(
        &mut self,
        mutations: Vec<Mutation>,
    ) -> Result<SceneVersion, RetainedSceneError> {
        let mut changes = SceneChangeSet::default();
        let mut undo = Vec::with_capacity(mutations.len());
        for mutation in mutations {
            match self.apply_with_undo(mutation, &mut changes) {
                Ok(entry) => undo.push(entry),
                Err(error) => {
                    for entry in undo.into_iter().rev() {
                        self.undo(entry);
                    }
                    return Err(error);
                }
            }
        }
        if changes == SceneChangeSet::default() {
            return Ok(self.version);
        }
        self.version = SceneVersion(self.version.0.wrapping_add(1));
        self.journal.push_back(JournalEntry {
            version: self.version,
            changes,
        });
        while self.journal.len() > JOURNAL_CAPACITY {
            self.journal.pop_front();
        }
        Ok(self.version)
    }

    fn apply_with_undo(
        &mut self,
        mutation: Mutation,
        changes: &mut SceneChangeSet,
    ) -> Result<UndoMutation, RetainedSceneError> {
        match mutation {
            Mutation::Insert {
                parent,
                before,
                id,
                kind,
            } => {
                if self.nodes.contains_key(&id) {
                    return Err(RetainedSceneError::DuplicateNode(id));
                }
                validate_kind(&kind, self.scale)?;
                let key = self.insert_child(parent, before, id)?;
                self.nodes.insert(
                    id,
                    SceneNode {
                        kind,
                        parent: Some(parent),
                        content: ChildList::default(),
                        mask: ChildList::default(),
                        generation: 0,
                    },
                );
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Insert { parent, id, key })
            }
            Mutation::ReplaceScene { id, canvas } => {
                validate_canvas(&canvas, self.scale)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                let NodeKind::Scene { position, .. } = &node.kind else {
                    return Err(RetainedSceneError::MissingNode(id));
                };
                let old_kind = node.kind.clone();
                let old_generation = node.generation;
                node.kind = NodeKind::Scene {
                    canvas,
                    position: *position,
                };
                node.generation = node.generation.wrapping_add(1);
                changes.changed_nodes.insert(id);
                Ok(UndoMutation::NodeValue {
                    id,
                    kind: old_kind,
                    generation: old_generation,
                })
            }
            Mutation::SetPosition { id, position } => {
                validate_position(position)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                let NodeKind::Scene {
                    canvas: current_canvas,
                    position: current,
                } = &node.kind
                else {
                    return Err(RetainedSceneError::MissingNode(id));
                };
                if *current != position {
                    let old_kind = node.kind.clone();
                    let old_generation = node.generation;
                    node.kind = NodeKind::Scene {
                        canvas: current_canvas.clone(),
                        position,
                    };
                    node.generation = node.generation.wrapping_add(1);
                    changes.changed_nodes.insert(id);
                    Ok(UndoMutation::NodeValue {
                        id,
                        kind: old_kind,
                        generation: old_generation,
                    })
                } else {
                    Ok(UndoMutation::None)
                }
            }
            Mutation::UpdateLayer { id, layer } => {
                validate_layer(&layer)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                if !matches!(node.kind, NodeKind::Layer(_)) {
                    return Err(RetainedSceneError::MissingNode(id));
                }
                if !matches!(layer, RetainedLayerDescriptor::Mask(_)) && !node.mask.is_empty() {
                    return Err(RetainedSceneError::InvalidParentBranch(id));
                }
                let old_kind = node.kind.clone();
                let old_generation = node.generation;
                node.kind = NodeKind::Layer(layer);
                node.generation = node.generation.wrapping_add(1);
                changes.changed_nodes.insert(id);
                changes.changed_layers.insert(id);
                changes.topology_changed = true;
                Ok(UndoMutation::NodeValue {
                    id,
                    kind: old_kind,
                    generation: old_generation,
                })
            }
            Mutation::Reparent { id, parent, before } => {
                if id == self.root {
                    return Err(RetainedSceneError::CannotRemoveRoot);
                }
                self.ensure_no_cycle(id, parent.node)?;
                let old_parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .expect("non-root node has parent");
                self.validate_child_insert(parent, before, id)?;
                let old_key = self.remove_child(old_parent, id)?;
                let new_key = self.insert_child(parent, before, id)?;
                self.nodes.get_mut(&id).unwrap().parent = Some(parent);
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Reparent {
                    id,
                    old_parent,
                    old_key,
                    new_parent: parent,
                    new_key,
                })
            }
            Mutation::MoveBefore { id, sibling } => {
                if id == sibling {
                    return Ok(UndoMutation::None);
                }
                let parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .ok_or(RetainedSceneError::CannotRemoveRoot)?;
                if self
                    .nodes
                    .get(&sibling)
                    .ok_or(RetainedSceneError::MissingNode(sibling))?
                    .parent
                    != Some(parent)
                {
                    return Err(RetainedSceneError::InvalidSibling(sibling));
                }
                let old_key = self.remove_child(parent, id)?;
                let new_key = self.insert_child(parent, Some(sibling), id)?;
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Reparent {
                    id,
                    old_parent: parent,
                    old_key,
                    new_parent: parent,
                    new_key,
                })
            }
            Mutation::Remove { id } => {
                if id == self.root {
                    return Err(RetainedSceneError::CannotRemoveRoot);
                }
                let parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .expect("non-root node has parent");
                let parent_key = self.remove_child(parent, id)?;
                let mut removed = Vec::new();
                self.collect_subtree(id, &mut removed);
                let mut removed_nodes = Vec::with_capacity(removed.len());
                for removed_id in removed {
                    let node = self.nodes.remove(&removed_id).unwrap();
                    removed_nodes.push((removed_id, node));
                    changes.removed_nodes.insert(removed_id);
                }
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Remove {
                    parent,
                    parent_key,
                    nodes: removed_nodes,
                })
            }
            Mutation::Resize {
                width,
                height,
                scale,
            } => {
                validate_size(width, height, scale)?;
                if self.nodes.values().any(|node| match &node.kind {
                    NodeKind::Scene { canvas, .. } => {
                        (canvas.scale_factor() - scale).abs() > f32::EPSILON
                    }
                    _ => false,
                }) {
                    return Err(RetainedSceneError::ScaleMismatch);
                }
                if (self.width, self.height, self.scale.to_bits())
                    != (width, height, scale.to_bits())
                {
                    let old = (self.width, self.height, self.scale);
                    self.width = width;
                    self.height = height;
                    self.scale = scale;
                    changes.surface_changed = true;
                    changes.invalidate_all = true;
                    Ok(UndoMutation::Resize(old))
                } else {
                    Ok(UndoMutation::None)
                }
            }
            Mutation::InvalidateRect(rect) => {
                if ![rect.x0, rect.y0, rect.x1, rect.y1]
                    .into_iter()
                    .all(f64::is_finite)
                {
                    return Err(RetainedSceneError::InvalidPosition);
                }
                if !rect.is_zero_area() {
                    changes.invalidated_rects.push(rect);
                }
                Ok(UndoMutation::None)
            }
            Mutation::InvalidateAll => {
                changes.invalidate_all = true;
                Ok(UndoMutation::None)
            }
        }
    }

    fn undo(&mut self, mutation: UndoMutation) {
        match mutation {
            UndoMutation::None => {}
            UndoMutation::Insert { parent, id, key } => {
                let removed = self.remove_child(parent, id);
                debug_assert_eq!(removed, Ok(key));
                self.nodes.remove(&id).expect("undo inserted node exists");
            }
            UndoMutation::NodeValue {
                id,
                kind,
                generation,
            } => {
                let node = self.nodes.get_mut(&id).expect("undo node exists");
                node.kind = kind;
                node.generation = generation;
            }
            UndoMutation::Reparent {
                id,
                old_parent,
                old_key,
                new_parent,
                new_key,
            } => {
                let removed = self.remove_child(new_parent, id);
                debug_assert_eq!(removed, Ok(new_key));
                self.insert_child_at(old_parent, old_key, id);
                self.nodes.get_mut(&id).unwrap().parent = Some(old_parent);
            }
            UndoMutation::Remove {
                parent,
                parent_key,
                nodes,
            } => {
                let root = nodes[0].0;
                self.nodes.extend(nodes);
                self.insert_child_at(parent, parent_key, root);
            }
            UndoMutation::Resize((width, height, scale)) => {
                (self.width, self.height, self.scale) = (width, height, scale);
            }
        }
    }

    fn insert_child(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<u128, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent.node, parent.branch)?
            .insert_before(id, before)
    }

    fn validate_child_insert(
        &self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        let children = self
            .nodes
            .get(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children(parent.branch)?;
        if let Some(before) = before
            && (before == id || children.key_of(before).is_none())
        {
            return Err(RetainedSceneError::InvalidSibling(before));
        }
        Ok(())
    }

    fn insert_child_at(&mut self, parent: RetainedParent, key: u128, id: RetainedNodeId) {
        self.nodes
            .get_mut(&parent.node)
            .expect("undo parent exists")
            .children_mut(parent.node, parent.branch)
            .expect("undo branch is valid")
            .insert_at(key, id);
    }

    fn remove_child(
        &mut self,
        parent: RetainedParent,
        id: RetainedNodeId,
    ) -> Result<u128, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent.node, parent.branch)?
            .remove(id)
            .ok_or(RetainedSceneError::MissingNode(id))
    }

    fn ensure_no_cycle(
        &self,
        id: RetainedNodeId,
        mut parent: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        loop {
            if parent == id {
                return Err(RetainedSceneError::Cycle(id));
            }
            let node = self
                .nodes
                .get(&parent)
                .ok_or(RetainedSceneError::MissingNode(parent))?;
            let Some(next) = node.parent else {
                return Ok(());
            };
            parent = next.node;
        }
    }

    fn collect_subtree(&self, id: RetainedNodeId, out: &mut Vec<RetainedNodeId>) {
        out.push(id);
        let node = &self.nodes[&id];
        for child in node.content.values().chain(node.mask.values()) {
            self.collect_subtree(*child, out);
        }
    }
}

enum Mutation {
    Insert {
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        kind: NodeKind,
    },
    ReplaceScene {
        id: RetainedNodeId,
        canvas: Arc<Canvas>,
    },
    SetPosition {
        id: RetainedNodeId,
        position: Point,
    },
    UpdateLayer {
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    },
    Reparent {
        id: RetainedNodeId,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
    },
    MoveBefore {
        id: RetainedNodeId,
        sibling: RetainedNodeId,
    },
    Remove {
        id: RetainedNodeId,
    },
    Resize {
        width: u32,
        height: u32,
        scale: f32,
    },
    InvalidateRect(Rect),
    InvalidateAll,
}

enum UndoMutation {
    None,
    Insert {
        parent: RetainedParent,
        id: RetainedNodeId,
        key: u128,
    },
    NodeValue {
        id: RetainedNodeId,
        kind: NodeKind,
        generation: u64,
    },
    Reparent {
        id: RetainedNodeId,
        old_parent: RetainedParent,
        old_key: u128,
        new_parent: RetainedParent,
        new_key: u128,
    },
    Remove {
        parent: RetainedParent,
        parent_key: u128,
        nodes: Vec<(RetainedNodeId, SceneNode)>,
    },
    Resize((u32, u32, f32)),
}

pub struct RetainedSceneTransaction<'a> {
    scene: &'a mut RetainedScene,
    mutations: Vec<Mutation>,
}

impl RetainedSceneTransaction<'_> {
    pub fn insert_scene(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        canvas: Arc<Canvas>,
        position: impl Into<Point>,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Scene {
                canvas,
                position: position.into(),
            },
        });
        self
    }

    pub fn insert_group(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Group,
        });
        self
    }

    pub fn insert_layer(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    ) -> &mut Self {
        self.mutations.push(Mutation::Insert {
            parent,
            before,
            id,
            kind: NodeKind::Layer(layer),
        });
        self
    }

    pub fn replace_scene(&mut self, id: RetainedNodeId, canvas: Arc<Canvas>) -> &mut Self {
        self.mutations.push(Mutation::ReplaceScene { id, canvas });
        self
    }

    pub fn set_position(&mut self, id: RetainedNodeId, position: impl Into<Point>) -> &mut Self {
        self.mutations.push(Mutation::SetPosition {
            id,
            position: position.into(),
        });
        self
    }

    pub fn update_layer(
        &mut self,
        id: RetainedNodeId,
        layer: RetainedLayerDescriptor,
    ) -> &mut Self {
        self.mutations.push(Mutation::UpdateLayer { id, layer });
        self
    }

    pub fn reparent(
        &mut self,
        id: RetainedNodeId,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
    ) -> &mut Self {
        self.mutations
            .push(Mutation::Reparent { id, parent, before });
        self
    }

    pub fn move_before(&mut self, id: RetainedNodeId, sibling: RetainedNodeId) -> &mut Self {
        self.mutations.push(Mutation::MoveBefore { id, sibling });
        self
    }

    pub fn remove_subtree(&mut self, id: RetainedNodeId) -> &mut Self {
        self.mutations.push(Mutation::Remove { id });
        self
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) -> &mut Self {
        self.mutations.push(Mutation::Resize {
            width,
            height,
            scale,
        });
        self
    }

    pub fn invalidate_rect(&mut self, rect: Rect) -> &mut Self {
        self.mutations.push(Mutation::InvalidateRect(rect));
        self
    }

    pub fn invalidate_all(&mut self) -> &mut Self {
        self.mutations.push(Mutation::InvalidateAll);
        self
    }

    pub fn commit(&mut self) -> Result<SceneVersion, RetainedSceneError> {
        self.scene
            .commit_mutations(std::mem::take(&mut self.mutations))
    }
}

fn validate_size(width: u32, height: u32, scale: f32) -> Result<(), RetainedSceneError> {
    (width > 0 && height > 0 && scale.is_finite() && scale > 0.0)
        .then_some(())
        .ok_or(RetainedSceneError::InvalidSize)
}

fn validate_position(position: Point) -> Result<(), RetainedSceneError> {
    (position.x.is_finite() && position.y.is_finite())
        .then_some(())
        .ok_or(RetainedSceneError::InvalidPosition)
}

fn validate_canvas(canvas: &Canvas, scale: f32) -> Result<(), RetainedSceneError> {
    if !canvas.is_closed_for_append() {
        return Err(RetainedSceneError::UnclosedCanvas);
    }
    ((canvas.scale_factor() - scale).abs() <= f32::EPSILON)
        .then_some(())
        .ok_or(RetainedSceneError::ScaleMismatch)
}

fn validate_kind(kind: &NodeKind, scale: f32) -> Result<(), RetainedSceneError> {
    match kind {
        NodeKind::Scene { canvas, position } => {
            validate_canvas(canvas, scale)?;
            validate_position(*position)
        }
        NodeKind::Layer(layer) => validate_layer(layer),
        NodeKind::Group => Ok(()),
    }
}

fn validate_layer(layer: &RetainedLayerDescriptor) -> Result<(), RetainedSceneError> {
    match layer {
        RetainedLayerDescriptor::ClipPath {
            path,
            transform,
            tolerance,
            ..
        }
        | RetainedLayerDescriptor::Isolate {
            path,
            transform,
            tolerance,
        }
        | RetainedLayerDescriptor::Opacity {
            path,
            transform,
            tolerance,
            ..
        }
        | RetainedLayerDescriptor::Blend {
            path,
            transform,
            tolerance,
            ..
        } => {
            validate_path(path)?;
            if !transform.as_coeffs().into_iter().all(f64::is_finite)
                || !tolerance.is_finite()
                || *tolerance < 0.0
            {
                return Err(RetainedSceneError::InvalidPosition);
            }
            if let RetainedLayerDescriptor::Opacity { opacity, .. } = layer
                && (!opacity.is_finite() || !(0.0..=1.0).contains(opacity))
            {
                return Err(RetainedSceneError::InvalidPosition);
            }
            Ok(())
        }
        RetainedLayerDescriptor::Filter { sample_region, .. }
        | RetainedLayerDescriptor::Backdrop { sample_region, .. } => validate_region(sample_region),
        RetainedLayerDescriptor::Mask(mask) => validate_region(&mask.region),
        RetainedLayerDescriptor::ClipSdf(sdf) => validate_sdf(*sdf),
    }
}

fn validate_sdf(sdf: Sdf) -> Result<(), RetainedSceneError> {
    let point = |point: Point| point.x.is_finite() && point.y.is_finite();
    let floats = |values: &[f32]| values.iter().all(|value| value.is_finite());
    let radius = |radius: crate::Radius| {
        floats(&[
            radius.top_left,
            radius.top_right,
            radius.bottom_left,
            radius.bottom_right,
        ])
    };
    let rect = |rect: crate::shared::sdf::rect::Rect| {
        point(rect.start) && point(rect.end) && radius(rect.radius)
    };
    let circle = |circle: crate::shared::sdf::circle::Circle| {
        point(circle.center) && circle.radius.is_finite()
    };
    let line = |line: crate::shared::sdf::line::Line| {
        point(line.start) && point(line.end) && line.width.is_finite()
    };
    let valid = match sdf {
        Sdf::Rect(value) => rect(value),
        Sdf::RectStroke(value) => {
            rect(value.rect)
                && floats(&[
                    value.widths.top,
                    value.widths.right,
                    value.widths.bottom,
                    value.widths.left,
                ])
        }
        Sdf::Circle(value) => circle(value),
        Sdf::CircleStroke(value) => circle(value.circle) && value.half_width.is_finite(),
        Sdf::Arc(value) => {
            point(value.center)
                && floats(&[
                    value.radius,
                    value.start_angle,
                    value.sweep_angle,
                    value.width,
                ])
        }
        Sdf::CandleStick(value) => floats(&[
            value.center_x,
            value.high_y,
            value.low_y,
            value.body_top_y,
            value.body_bottom_y,
        ]),
        Sdf::Line(value) => line(value),
        Sdf::DashLine(value) => {
            line(value.line) && floats(&[value.dash_length, value.gap_length, value.dash_offset])
        }
    };
    valid
        .then_some(())
        .ok_or(RetainedSceneError::InvalidPosition)
}

fn validate_region(region: &Region) -> Result<(), RetainedSceneError> {
    match region {
        Region::Rect { rect, .. } => [rect.x0, rect.y0, rect.x1, rect.y1]
            .into_iter()
            .all(f64::is_finite)
            .then_some(())
            .ok_or(RetainedSceneError::InvalidPosition),
        Region::Path {
            path,
            transform,
            tolerance,
        } => {
            validate_path(path)?;
            (transform.as_coeffs().into_iter().all(f64::is_finite)
                && tolerance.is_finite()
                && *tolerance >= 0.0)
                .then_some(())
                .ok_or(RetainedSceneError::InvalidPosition)
        }
    }
}

fn validate_path(path: &BezPath) -> Result<(), RetainedSceneError> {
    let finite = |point: Point| point.x.is_finite() && point.y.is_finite();
    path.elements()
        .iter()
        .all(|element| match *element {
            PathEl::MoveTo(point) | PathEl::LineTo(point) => finite(point),
            PathEl::QuadTo(a, b) => finite(a) && finite(b),
            PathEl::CurveTo(a, b, c) => finite(a) && finite(b) && finite(c),
            PathEl::ClosePath => true,
        })
        .then_some(())
        .ok_or(RetainedSceneError::InvalidPosition)
}

#[cfg(test)]
mod tests {
    use peniko::{Color, kurbo::Shape};

    use super::*;
    use crate::Radius;

    fn leaf(color: Color) -> Arc<Canvas> {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
        Arc::new(canvas)
    }

    fn empty_leaf() -> Arc<Canvas> {
        Arc::new(Canvas::new(16, 16, 1.0))
    }

    fn backdrop_leaf() -> Arc<Canvas> {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_backdrop_layer(
            Filter::Opacity(0.5),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
        );
        canvas.pop_layer();
        Arc::new(canvas)
    }

    #[test]
    fn transaction_is_atomic_when_a_late_mutation_is_invalid() {
        let root = RetainedNodeId::for_owner(1);
        let child = RetainedNodeId::for_owner(2);
        let missing = RetainedNodeId::for_owner(9);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction
            .insert_scene(
                RetainedParent::content(root),
                None,
                child,
                leaf(Color::WHITE),
                (0.0, 0.0),
            )
            .reparent(child, RetainedParent::content(missing), None);
        assert_eq!(
            transaction.commit(),
            Err(RetainedSceneError::MissingNode(missing))
        );
        assert_eq!(scene.version(), SceneVersion::INITIAL);
        assert!(!scene.nodes.contains_key(&child));
    }

    #[test]
    fn content_revision_reuses_chunk_canvas_storage() {
        let root = RetainedNodeId::for_owner(5);
        let child = RetainedNodeId::for_owner(6);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                child,
                leaf(Color::WHITE),
                (8.0, 8.0),
            )
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let chunk_storage = std::ptr::from_ref(&materializer.chunks[&child]);
        let storage = std::ptr::from_ref(&materializer.chunks[&child].canvas);

        scene
            .transaction()
            .replace_scene(child, leaf(Color::BLACK))
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
        assert_eq!(
            std::ptr::from_ref(&materializer.chunks[&child]),
            chunk_storage
        );
        assert_eq!(
            std::ptr::from_ref(&materializer.chunks[&child].canvas),
            storage
        );
        assert_eq!(materializer.chunks[&child].generation, 1);
    }

    #[test]
    fn scene_content_replacement_refreshes_embedded_backdrop_index() {
        let root = RetainedNodeId::for_owner(60_000);
        let child = RetainedNodeId::for_owner(60_001);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                child,
                leaf(Color::WHITE),
                (8.0, 12.0),
            )
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        assert!(materializer.dependency_free);
        assert!(!materializer.nonlocal_dependencies.contains(&child));

        scene
            .transaction()
            .replace_scene(child, backdrop_leaf())
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
        assert!(!materializer.dependency_free);
        assert!(materializer.nonlocal_dependencies.contains(&child));
        assert_eq!(
            materializer.chunks[&child].backdrop_dependencies[0].output,
            Bounds::new(8, 12, 24, 28)
        );

        scene
            .transaction()
            .replace_scene(child, leaf(Color::BLACK))
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
        assert!(materializer.dependency_free);
        assert!(!materializer.nonlocal_dependencies.contains(&child));
    }

    #[test]
    fn empty_chunk_allocations_grow_and_shrink_without_full_sync() {
        let root = RetainedNodeId::for_owner(7);
        let child = RetainedNodeId::for_owner(8);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                child,
                empty_leaf(),
                (4.0, 4.0),
            )
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let chunk = &materializer.chunks[&child];
        assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
        assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

        scene
            .transaction()
            .replace_scene(child, leaf(Color::WHITE))
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
        let chunk = &materializer.chunks[&child];
        assert_eq!(materializer.arenas.draws.range(chunk.draws).len(), 1);
        assert!(!materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

        scene
            .transaction()
            .replace_scene(child, empty_leaf())
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
        let chunk = &materializer.chunks[&child];
        assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
        assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());
    }

    #[test]
    fn undo_log_restores_values_order_removals_and_surface_after_late_failure() {
        let root = RetainedNodeId::for_owner(10);
        let a = RetainedNodeId::for_owner(11);
        let b = RetainedNodeId::for_owner(12);
        let invalid = RetainedNodeId::for_owner(13);
        let original = leaf(Color::WHITE);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                a,
                original.clone(),
                (0.0, 0.0),
            )
            .insert_scene(
                RetainedParent::content(root),
                None,
                b,
                leaf(Color::BLACK),
                (16.0, 0.0),
            )
            .commit()
            .unwrap();
        let version = scene.version();
        let order = scene.nodes[&root]
            .content
            .order
            .iter()
            .map(|(&key, &id)| (key, id))
            .collect::<Vec<_>>();

        let result = scene
            .transaction()
            .replace_scene(a, leaf(Color::from_rgb8(20, 80, 220)))
            .move_before(b, a)
            .remove_subtree(b)
            .resize(96, 80, 1.0)
            .insert_scene(
                RetainedParent::mask(root),
                None,
                invalid,
                leaf(Color::WHITE),
                (0.0, 0.0),
            )
            .commit();

        assert_eq!(result, Err(RetainedSceneError::InvalidParentBranch(root)));
        assert_eq!(scene.version(), version);
        assert_eq!((scene.width, scene.height, scene.scale), (64, 64, 1.0));
        assert!(scene.nodes.contains_key(&b));
        assert!(!scene.nodes.contains_key(&invalid));
        assert_eq!(
            scene.nodes[&root]
                .content
                .order
                .iter()
                .map(|(&key, &id)| (key, id))
                .collect::<Vec<_>>(),
            order
        );
        let NodeKind::Scene { canvas, .. } = &scene.nodes[&a].kind else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(canvas, &original));
    }

    #[test]
    fn reparent_rejects_cycles() {
        let root = RetainedNodeId::for_owner(1);
        let a = RetainedNodeId::for_owner(2);
        let b = RetainedNodeId::for_owner(3);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction
            .insert_group(RetainedParent::content(root), None, a)
            .insert_group(RetainedParent::content(a), None, b);
        transaction.commit().unwrap();
        let mut transaction = scene.transaction();
        transaction.reparent(a, RetainedParent::content(b), None);
        assert_eq!(transaction.commit(), Err(RetainedSceneError::Cycle(a)));
    }

    #[test]
    fn move_before_changes_materialized_painter_order() {
        let root = RetainedNodeId::for_owner(1);
        let a = RetainedNodeId::for_owner(2);
        let b = RetainedNodeId::for_owner(3);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction
            .insert_scene(
                RetainedParent::content(root),
                None,
                a,
                leaf(Color::WHITE),
                (0.0, 0.0),
            )
            .insert_scene(
                RetainedParent::content(root),
                None,
                b,
                leaf(Color::BLACK),
                (0.0, 0.0),
            );
        transaction.commit().unwrap();
        let mut transaction = scene.transaction();
        transaction.move_before(b, a).commit().unwrap();
        let frame = scene.to_canvas().retained_frame().unwrap();
        assert_eq!(
            frame.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![b, a]
        );
    }

    #[test]
    fn mask_branch_is_only_valid_for_masks() {
        let root = RetainedNodeId::for_owner(1);
        let group = RetainedNodeId::for_owner(2);
        let child = RetainedNodeId::for_owner(3);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction
            .insert_group(RetainedParent::content(root), None, group)
            .insert_scene(
                RetainedParent::mask(group),
                None,
                child,
                leaf(Color::WHITE),
                (0.0, 0.0),
            );
        assert_eq!(
            transaction.commit(),
            Err(RetainedSceneError::InvalidParentBranch(group))
        );
    }

    #[test]
    fn layer_geometry_rejects_non_finite_coordinates_atomically() {
        let root = RetainedNodeId::for_owner(20);
        let layer = RetainedNodeId::for_owner(21);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let descriptor = RetainedLayerDescriptor::Opacity {
            path: Rect::new(0.0, 0.0, f64::NAN, 16.0).to_path(0.1),
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            opacity: 0.5,
        };

        assert_eq!(
            scene
                .transaction()
                .insert_layer(RetainedParent::content(root), None, layer, descriptor)
                .commit(),
            Err(RetainedSceneError::InvalidPosition)
        );
        assert_eq!(scene.version(), SceneVersion::INITIAL);
        assert!(!scene.nodes.contains_key(&layer));

        let descriptor =
            RetainedLayerDescriptor::ClipSdf(Sdf::Circle(crate::shared::sdf::circle::Circle {
                center: Point::new(f64::NAN, 8.0),
                radius: 4.0,
            }));
        assert_eq!(
            scene
                .transaction()
                .insert_layer(RetainedParent::content(root), None, layer, descriptor)
                .commit(),
            Err(RetainedSceneError::InvalidPosition)
        );
        assert_eq!(scene.version(), SceneVersion::INITIAL);
        assert!(!scene.nodes.contains_key(&layer));
    }

    #[test]
    fn journal_merges_skipped_versions() {
        let root = RetainedNodeId::for_owner(1);
        let child = RetainedNodeId::for_owner(2);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            (0.0, 0.0),
        );
        transaction.commit().unwrap();
        let mut transaction = scene.transaction();
        transaction.set_position(child, (4.0, 5.0));
        transaction.commit().unwrap();
        let changes = scene.changes_since(SceneVersion::INITIAL).unwrap();
        assert!(changes.changed_nodes.contains(&child));
        assert!(changes.topology_changed);
    }

    #[test]
    fn journal_gap_requires_one_full_resynchronization() {
        let root = RetainedNodeId::for_owner(1);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        for _ in 0..=JOURNAL_CAPACITY {
            scene
                .transaction()
                .invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
                .commit()
                .unwrap();
        }

        assert!(scene.changes_since(SceneVersion::INITIAL).is_none());
        let recent = SceneVersion(scene.version().get() - 1);
        assert!(scene.changes_since(recent).is_some());
    }

    #[test]
    fn appended_root_layer_fragment_is_visible_in_cached_execution_plan() {
        let root = RetainedNodeId::for_owner(30);
        let base = RetainedNodeId::for_owner(31);
        let layer = RetainedNodeId::for_owner(32);
        let child = RetainedNodeId::for_owner(33);
        let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                base,
                leaf(Color::from_rgb8(20, 40, 220)),
                (0.0, 0.0),
            )
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let base_batch = materializer.node_batches[&base];
        let base_frame = materializer.canvas.retained_frame_override.clone().unwrap();
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                RetainedLayerDescriptor::Opacity {
                    path: Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                    opacity: 0.5,
                },
            )
            .insert_scene(
                RetainedParent::content(layer),
                None,
                child,
                leaf(Color::from_rgb8(220, 40, 20)),
                (0.0, 0.0),
            )
            .commit()
            .unwrap();

        assert!(materializer.update(&scene));
        assert_eq!(materializer.node_batches[&base], base_batch);
        let plan = materializer
            .canvas
            .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, crate::shared::execution::ExecOp::DrawBatch { .. }))
                .count(),
            2,
            "patched root plan: {:#?}",
            plan.ops
        );
        let inserted_frame = materializer.canvas.retained_frame_override.clone().unwrap();
        assert!(
            Arc::ptr_eq(&base_frame.nodes, &inserted_frame.nodes),
            "root layer insertion must patch the immutable frame instead of collecting every node"
        );
        assert!(inserted_frame.node_state(layer).is_some());
        assert!(inserted_frame.node_state(child).is_some());
        assert_eq!(inserted_frame.delta.as_ref().unwrap().depth, 1);

        scene.transaction().remove_subtree(layer).commit().unwrap();
        assert!(materializer.update(&scene));
        let removed_frame = materializer.canvas.retained_frame_override.clone().unwrap();
        assert!(Arc::ptr_eq(&base_frame.nodes, &removed_frame.nodes));
        assert!(removed_frame.node_state(layer).is_none());
        assert!(removed_frame.node_state(child).is_none());
        assert_eq!(removed_frame.delta.as_ref().unwrap().depth, 1);
    }
}
