use std::{collections::HashSet, sync::Arc};

use peniko::kurbo::{Point, Rect};
use rustc_hash::FxHashMap as HashMap;

use super::{Canvas, SceneAppendMode, SceneOffset};
use crate::{
    RetainedParent, RetainedScene,
    retained_scene::PersistentSceneMaterializer,
    shared::{
        bounds::Bounds,
        execution::Command,
        layer::{Layer, filter, region::Region},
    },
};

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

/// Monotonic version of the commands owned by a retained node.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SceneRevision(u64);

impl SceneRevision {
    pub const INITIAL: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn advance(&mut self) -> Self {
        self.0 = self.0.wrapping_add(1);
        *self
    }
}

impl From<u64> for SceneRevision {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

/// Stable identity and revision for a retained layer boundary.
///
/// A key belongs to the visual operation opened by `push_*_retained_layer`,
/// not to a generic widget/container. This lets the renderer retain clips and
/// offscreen surfaces without fragmenting ordinary draw batches. Callers must
/// advance `revision` whenever the layer, its direct commands, or other
/// non-retained content owned by the layer changes; retained collection never
/// hashes command contents.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RetainedLayerKey {
    pub id: RetainedNodeId,
    pub revision: SceneRevision,
}

impl RetainedLayerKey {
    pub const fn new(id: RetainedNodeId, revision: SceneRevision) -> Self {
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
    pub(crate) revision: SceneRevision,
    pub(crate) bounds: Bounds,
    pub(crate) order: u32,
    pub(crate) kind: RetainedNodeKind,
    pub(crate) placement_bits: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetainedNodePatch {
    pub(crate) old: Option<RetainedNodeState>,
    pub(crate) new: Option<RetainedNodeState>,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFrameDelta {
    pub(crate) from_version: u64,
    pub(crate) to_version: u64,
    pub(crate) patches: Arc<[RetainedNodePatch]>,
    pub(crate) previous: Option<Arc<RetainedFrameDelta>>,
    pub(crate) depth: u16,
    pub(crate) damage: Arc<[(RetainedNodeId, Bounds)]>,
    /// Backdrops affected by this persistent journal delta. When complete, the renderer can skip
    /// the generic command-tree propagation pass because `damage` already contains their output.
    pub(crate) dirty_backdrops: Arc<[RetainedNodeId]>,
    pub(crate) backdrop_damage_complete: bool,
    pub(crate) index: Arc<HashMap<RetainedNodeId, usize>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFrame {
    pub(crate) root: RetainedNodeId,
    pub(crate) logical_size: (u32, u32),
    pub(crate) physical_size: (u32, u32),
    pub(crate) scale_bits: u32,
    pub(crate) nodes: Arc<[RetainedNodeState]>,
    pub(crate) node_index: Arc<HashMap<RetainedNodeId, usize>>,
    pub(crate) invalidated_bounds: Vec<Bounds>,
    pub(crate) invalidate_all: bool,
    /// Whether every command is represented by retained identity and the flattened scene can be
    /// reused solely from node metadata. Manual damage can make a frame incrementally complete,
    /// but cannot make untracked command contents safe to cache.
    pub(crate) materialization_cacheable: bool,
    pub(crate) incremental_complete: bool,
    /// Stateful-scene version and small immutable overlay. Snapshot canvases leave these empty.
    pub(crate) version: Option<u64>,
    pub(crate) delta: Option<Arc<RetainedFrameDelta>>,
    /// No layer/filter/mask can propagate leaf damage outside the changed node bounds.
    pub(crate) dependency_free: bool,
    /// Backdrops require walking command ancestry after retained diffing to discover changed
    /// sampled background. Filter descendants already carry filter-expanded frame bounds; manual
    /// invalidation is handled separately because it has no retained node attribution.
    pub(crate) requires_damage_propagation: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RetainedDamage {
    node_bounds: HashMap<RetainedNodeId, Bounds>,
    unattributed: Vec<Bounds>,
}

pub(crate) struct RetainedDamagePropagation {
    pub(crate) bounds: Vec<Bounds>,
    pub(crate) dirty_backdrops: HashSet<RetainedNodeId>,
}

impl RetainedDamage {
    pub(crate) fn add_node(&mut self, id: RetainedNodeId, bounds: Bounds) {
        if bounds.is_empty() {
            return;
        }
        self.node_bounds
            .entry(id)
            .and_modify(|current| *current = current.union(bounds))
            .or_insert(bounds);
    }

    pub(crate) fn add_unattributed(&mut self, bounds: Bounds) {
        push_unique_damage(&mut self.unattributed, bounds);
    }
}

impl RetainedFrame {
    pub(crate) fn node_state(&self, id: RetainedNodeId) -> Option<RetainedNodeState> {
        let mut delta = self.delta.as_deref();
        while let Some(current) = delta {
            if let Some(&index) = current.index.get(&id) {
                return current.patches[index].new;
            }
            delta = current.previous.as_deref();
        }
        self.node_index.get(&id).map(|&index| self.nodes[index])
    }

    pub(crate) fn node_revision(&self, id: RetainedNodeId) -> Option<SceneRevision> {
        self.node_state(id).map(|node| node.revision)
    }

    pub(crate) fn same_scene(&self, other: &Self) -> bool {
        self.root == other.root
            && self.logical_size == other.logical_size
            && self.physical_size == other.physical_size
            && self.scale_bits == other.scale_bits
            && self.nodes == other.nodes
            && self.materialization_cacheable == other.materialization_cacheable
            && self.incremental_complete == other.incremental_complete
    }
}

#[derive(Default)]
pub(crate) struct RetainedSceneCache {
    /// One entry per node makes revision replacement O(1). The previous composite-key map had to
    /// scan every cached scene before each lookup to remove older revisions, making materializing
    /// an N-node frame O(N²).
    scenes: HashMap<RetainedNodeId, CachedRetainedScene>,
    snapshot: Option<Box<SnapshotMaterialization>>,
}

struct CachedRetainedScene {
    revision: SceneRevision,
    canvas: Arc<Canvas>,
}

#[derive(Clone)]
struct SnapshotNode {
    id: RetainedNodeId,
    revision: SceneRevision,
    canvas: Arc<Canvas>,
    position: Point,
}

struct SnapshotMaterialization {
    root: RetainedNodeId,
    logical_size: (u32, u32),
    scale_bits: u32,
    nodes: HashMap<RetainedNodeId, (SceneRevision, Point)>,
    order: Vec<RetainedNodeId>,
    scene: RetainedScene,
    materializer: PersistentSceneMaterializer,
}

impl RetainedSceneCache {
    fn scene(
        &mut self,
        id: RetainedNodeId,
        revision: SceneRevision,
        canvas: &Arc<Canvas>,
    ) -> Arc<Canvas> {
        let cached = self
            .scenes
            .entry(id)
            .or_insert_with(|| CachedRetainedScene {
                revision,
                canvas: canvas.clone(),
            });
        if cached.revision != revision {
            *cached = CachedRetainedScene {
                revision,
                canvas: canvas.clone(),
            };
        }
        cached.canvas.clone()
    }

    pub(crate) fn retain_frame(&mut self, frame: &RetainedFrame) {
        let active = frame
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<HashSet<_>>();
        self.scenes.retain(|id, _| active.contains(id));
    }

    /// Adapts flat snapshot-style retained children to the same stable chunk backend used by
    /// [`RetainedScene`]. Metadata is still scanned because snapshots carry no journal, while
    /// unchanged child records remain in their arena allocations.
    pub(crate) fn materialize_snapshot_shared(&mut self, canvas: &Canvas) -> Arc<Canvas> {
        let Some(nodes) = flat_snapshot_nodes(canvas) else {
            return Arc::new(canvas.materialize_retained_scenes(self));
        };
        let root = canvas
            .retained_root
            .expect("snapshot adapter requires retained root");
        let compatible = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.root == root
                && snapshot.logical_size == canvas.logical_size()
                && snapshot.scale_bits == canvas.scale_factor.to_bits()
        });
        if !compatible {
            self.snapshot = Some(Box::new(SnapshotMaterialization::new(canvas, nodes)));
        } else {
            self.snapshot.as_mut().unwrap().update(canvas, nodes);
        }
        self.snapshot.as_ref().unwrap().materializer.canvas()
    }
}

impl SnapshotMaterialization {
    fn new(canvas: &Canvas, nodes: Vec<SnapshotNode>) -> Self {
        let root = canvas.retained_root.expect("snapshot has retained root");
        let mut scene = RetainedScene::new(
            canvas.logical_width,
            canvas.logical_height,
            canvas.scale_factor,
            root,
        )
        .expect("validated Canvas dimensions are a valid retained scene");
        let mut transaction = scene.transaction();
        for node in &nodes {
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                node.id,
                node.canvas.clone(),
                node.position,
            );
        }
        append_snapshot_invalidation(&mut transaction, canvas);
        transaction
            .commit()
            .expect("Canvas retained snapshot was validated when recorded");
        let materializer = PersistentSceneMaterializer::new(&scene);
        Self {
            root,
            logical_size: canvas.logical_size(),
            scale_bits: canvas.scale_factor.to_bits(),
            nodes: snapshot_node_map(&nodes),
            order: nodes.iter().map(|node| node.id).collect(),
            scene,
            materializer,
        }
    }

    fn update(&mut self, canvas: &Canvas, nodes: Vec<SnapshotNode>) {
        let desired = snapshot_node_map(&nodes);
        let desired_order = nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let mut transaction = self.scene.transaction();
        for id in self.nodes.keys().filter(|id| !desired.contains_key(id)) {
            transaction.remove_subtree(*id);
        }
        for node in &nodes {
            match self.nodes.get(&node.id) {
                None => {
                    transaction.insert_scene(
                        RetainedParent::content(self.root),
                        None,
                        node.id,
                        node.canvas.clone(),
                        node.position,
                    );
                }
                Some((revision, position)) => {
                    if *revision != node.revision {
                        transaction.replace_scene(node.id, node.canvas.clone());
                    }
                    if *position != node.position {
                        transaction.set_position(node.id, node.position);
                    }
                }
            }
        }
        if self.order != desired_order {
            let mut effective_order = self
                .order
                .iter()
                .copied()
                .filter(|id| desired.contains_key(id))
                .collect::<Vec<_>>();
            effective_order.extend(
                desired_order
                    .iter()
                    .copied()
                    .filter(|id| !self.nodes.contains_key(id)),
            );
            for (id, before) in snapshot_reorder_moves(&effective_order, &desired_order) {
                transaction.move_before(id, before);
            }
        }
        append_snapshot_invalidation(&mut transaction, canvas);
        transaction
            .commit()
            .expect("Canvas retained snapshot was validated when recorded");
        self.nodes = desired;
        self.order = desired_order;
        self.materializer.update(&self.scene);
    }
}

fn flat_snapshot_nodes(canvas: &Canvas) -> Option<Vec<SnapshotNode>> {
    if canvas.retained_root.is_none() || canvas.command_lists.len() != 1 {
        return None;
    }
    let mut ids = HashSet::new();
    canvas.command_lists[canvas.root_commands]
        .commands
        .iter()
        .map(|command| {
            let Command::RetainedScene {
                id,
                revision,
                canvas: child,
                offset,
            } = command
            else {
                return None;
            };
            ids.insert(*id).then_some(SnapshotNode {
                id: *id,
                revision: *revision,
                canvas: child.clone(),
                position: Point::new(
                    offset.0 / f64::from(canvas.scale_factor),
                    offset.1 / f64::from(canvas.scale_factor),
                ),
            })
        })
        .collect()
}

fn snapshot_node_map(nodes: &[SnapshotNode]) -> HashMap<RetainedNodeId, (SceneRevision, Point)> {
    nodes
        .iter()
        .map(|node| (node.id, (node.revision, node.position)))
        .collect()
}

fn snapshot_reorder_moves(
    current: &[RetainedNodeId],
    desired: &[RetainedNodeId],
) -> Vec<(RetainedNodeId, RetainedNodeId)> {
    let mut previous = HashMap::with_capacity_and_hasher(current.len(), Default::default());
    let mut next = HashMap::with_capacity_and_hasher(current.len(), Default::default());
    for (index, &id) in current.iter().enumerate() {
        previous.insert(id, index.checked_sub(1).map(|index| current[index]));
        next.insert(id, current.get(index + 1).copied());
    }
    let mut moves = Vec::new();
    for pair in desired.windows(2).rev() {
        let (id, before) = (pair[0], pair[1]);
        if next.get(&id).copied().flatten() == Some(before) {
            continue;
        }
        let old_previous = previous[&id];
        let old_next = next[&id];
        if let Some(old_previous) = old_previous {
            next.insert(old_previous, old_next);
        }
        if let Some(old_next) = old_next {
            previous.insert(old_next, old_previous);
        }
        let insert_previous = previous[&before];
        previous.insert(id, insert_previous);
        next.insert(id, Some(before));
        previous.insert(before, Some(id));
        if let Some(insert_previous) = insert_previous {
            next.insert(insert_previous, Some(id));
        }
        moves.push((id, before));
    }
    moves
}

fn append_snapshot_invalidation(
    transaction: &mut crate::RetainedSceneTransaction<'_>,
    canvas: &Canvas,
) {
    if canvas.invalidate_all {
        transaction.invalidate_all();
    }
    let scale = f64::from(canvas.scale_factor);
    for bounds in &canvas.invalidated_bounds {
        transaction.invalidate_rect(Rect::new(
            f64::from(bounds.x0) / scale,
            f64::from(bounds.y0) / scale,
            f64::from(bounds.x1) / scale,
            f64::from(bounds.y1) / scale,
        ));
    }
}

impl Canvas {
    /// Appends a reusable child scene while preserving its identity until render preparation.
    ///
    /// `revision` is the explicit content identity. Callers must advance it
    /// whenever `scene` changes; retained collection does not compare or hash
    /// child commands.
    pub fn append_retained_scene(
        &mut self,
        id: RetainedNodeId,
        revision: impl Into<SceneRevision>,
        scene: Arc<Canvas>,
        pos: impl Into<Point>,
    ) {
        self.ensure_command_root();
        assert!(
            scene.command_stack.len() == 1 && scene.layer_stack.is_empty(),
            "cannot append a canvas with unclosed layers"
        );
        assert!(
            (self.scale_factor - scene.scale_factor).abs() <= f32::EPSILON,
            "cannot append canvases with different scale factors"
        );
        if !self.is_retained() {
            self.append(&scene, pos);
            return;
        }
        let offset = SceneOffset::new(self.physical_point(pos.into()));
        self.current_command_list_mut()
            .commands
            .push(Command::RetainedScene {
                id,
                revision: revision.into(),
                canvas: scene,
                offset: (offset.dx, offset.dy),
            });
    }

    /// Adds caller-supplied damage in logical canvas coordinates.
    ///
    /// This can track direct commands that are not enclosed by retained identity. Callers must
    /// invalidate both old and new affected bounds whenever those commands change or disappear.
    /// Manual damage enables incremental rendering but never makes untracked commands eligible
    /// for materialized-scene reuse.
    pub fn invalidate_rect(&mut self, rect: Rect) {
        assert!(
            rect.x0.is_finite()
                && rect.y0.is_finite()
                && rect.x1.is_finite()
                && rect.y1.is_finite(),
            "invalidated rectangle must be finite"
        );
        if rect.is_zero_area() {
            return;
        }
        let rect = self.physical_rect(rect);
        let bounds = Bounds::new(
            rect.x0.floor() as i32,
            rect.y0.floor() as i32,
            rect.x1.ceil() as i32,
            rect.y1.ceil() as i32,
        )
        .intersect(Bounds::canvas(
            self.physical_width(),
            self.physical_height(),
        ));
        if !bounds.is_empty() {
            self.invalidated_bounds.push(bounds);
        }
    }

    /// Marks the whole output damaged without making untracked commands cacheable.
    pub fn invalidate_all(&mut self) {
        self.invalidate_all = true;
    }

    pub(crate) fn retained_frame(&self) -> Option<RetainedFrame> {
        if let Some(frame) = &self.retained_frame_override {
            return Some(frame.clone());
        }
        self.collect_retained_frame()
    }

    pub(crate) fn collect_retained_frame(&self) -> Option<RetainedFrame> {
        let root = self.retained_root?;
        let mut collector = FrameCollector::new(self);
        collector.visit_list(
            self,
            self.root_commands,
            false,
            SceneOffset { dx: 0.0, dy: 0.0 },
        );
        let nodes = collector.nodes;
        let node_index = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        Some(RetainedFrame {
            root,
            logical_size: self.logical_size(),
            physical_size: self.physical_size(),
            scale_bits: self.scale_factor.to_bits(),
            nodes: nodes.into(),
            node_index: Arc::new(node_index),
            invalidated_bounds: self.invalidated_bounds.clone(),
            invalidate_all: self.invalidate_all,
            materialization_cacheable: collector.identity_complete,
            incremental_complete: collector.identity_complete
                || self.invalidate_all
                || !self.invalidated_bounds.is_empty(),
            version: None,
            delta: None,
            dependency_free: false,
            requires_damage_propagation: true,
        })
    }

    pub(crate) fn visual_bounds(&self) -> Bounds {
        canvas_visual_bounds(self)
    }

    pub(crate) fn materialize_retained_scenes(&self, cache: &mut RetainedSceneCache) -> Canvas {
        if !self.has_retained_scenes() {
            return self.clone();
        }

        let mut materialized = self.clone();
        let mut list_ix = 0;
        while list_ix < materialized.command_lists.len() {
            let commands = std::mem::take(&mut materialized.command_lists[list_ix].commands);
            for command in commands {
                match command {
                    Command::RetainedScene {
                        id,
                        revision,
                        canvas,
                        offset,
                    } => {
                        let retained = cache.scene(id, revision, &canvas);
                        let children = materialized
                            .append_scene_ref_to_list_unchecked(
                                &retained,
                                list_ix,
                                SceneAppendMode::AppendAsCommandList,
                                SceneOffset {
                                    dx: offset.0,
                                    dy: offset.1,
                                },
                            )
                            .expect("retained scene append creates a command list");
                        materialized.command_lists[list_ix].commands.push(
                            Command::MaterializedRetainedScene {
                                id,
                                revision,
                                children,
                            },
                        );
                    }
                    command => materialized.command_lists[list_ix].commands.push(command),
                }
            }
            list_ix += 1;
        }
        materialized
    }

    /// Propagates already-known output damage through filter dependencies.
    ///
    /// The retained diff identifies changed component pixels. This pass adds
    /// pixels whose value depends on those changes, notably blur/shadow output
    /// and backdrop regions. It is deliberately conservative for graph filters:
    /// an uncertain dependency redraws that layer, never unrelated root tiles.
    pub(crate) fn propagate_damage(&self, damage: &RetainedDamage) -> RetainedDamagePropagation {
        let mut propagated = damage.unattributed.clone();
        let mut dirty_backdrops = HashSet::new();
        propagate_list_damage(
            self,
            self.root_commands,
            damage,
            &mut propagated,
            &mut dirty_backdrops,
            None,
        );
        RetainedDamagePropagation {
            bounds: propagated,
            dirty_backdrops,
        }
    }

    pub(crate) fn has_retained_scenes(&self) -> bool {
        self.command_lists.iter().any(|list| {
            list.commands
                .iter()
                .any(|command| matches!(command, Command::RetainedScene { .. }))
        })
    }
}

struct FrameCollector {
    canvas_bounds: Bounds,
    nodes: Vec<RetainedNodeState>,
    ids: HashSet<RetainedNodeId>,
    order: u32,
    identity_complete: bool,
}

impl FrameCollector {
    fn new(canvas: &Canvas) -> Self {
        Self {
            canvas_bounds: Bounds::canvas(canvas.physical_width(), canvas.physical_height()),
            nodes: Vec::new(),
            ids: HashSet::new(),
            order: 0,
            identity_complete: true,
        }
    }

    fn push_node(
        &mut self,
        id: RetainedNodeId,
        revision: SceneRevision,
        bounds: Bounds,
        kind: RetainedNodeKind,
        placement_bits: Option<(u64, u64)>,
    ) {
        if !self.ids.insert(id) {
            self.identity_complete = false;
            return;
        }
        self.nodes.push(RetainedNodeState {
            id,
            revision,
            // Keep source bounds outside the root canvas until ancestor
            // filters have propagated their sampling influence. A blur can
            // sample an off-canvas child and still change visible edge pixels.
            bounds,
            order: self.order,
            kind,
            placement_bits,
        });
        self.order += 1;
    }

    fn visit_list(
        &mut self,
        canvas: &Canvas,
        list_id: usize,
        inside_retained: bool,
        offset: SceneOffset,
    ) -> Bounds {
        let mut bounds = empty_bounds();
        for command in &canvas.command_lists[list_id].commands {
            let command_bounds = match command {
                Command::Draw(draw) => {
                    if !inside_retained {
                        self.identity_complete = false;
                    }
                    offset.bounds(pixel_bounds(canvas.draw_records[*draw].pixel_bounds))
                }
                Command::RetainedScene {
                    id,
                    revision,
                    canvas: child,
                    offset: child_offset,
                } => {
                    let child_offset = SceneOffset {
                        dx: offset.dx + child_offset.0,
                        dy: offset.dy + child_offset.1,
                    };
                    let child_bounds = child_offset.bounds(canvas_visual_bounds(child));
                    self.push_node(
                        *id,
                        *revision,
                        child_bounds,
                        RetainedNodeKind::Scene,
                        Some((child_offset.dx.to_bits(), child_offset.dy.to_bits())),
                    );
                    child_bounds
                }
                Command::MaterializedRetainedScene {
                    id,
                    revision,
                    children,
                } => {
                    let child_bounds = self.visit_list(canvas, *children, true, offset);
                    self.push_node(*id, *revision, child_bounds, RetainedNodeKind::Scene, None);
                    child_bounds
                }
                Command::Layer {
                    retained,
                    draw,
                    layer,
                    children,
                } => {
                    let owns_identity = retained.is_some();
                    if !inside_retained && !owns_identity {
                        self.identity_complete = false;
                    }
                    let node_start = self.nodes.len();
                    let child_bounds = self.visit_list(
                        canvas,
                        *children,
                        inside_retained || owns_identity,
                        offset,
                    );
                    let bounds = layer_bounds(canvas, *draw, layer, child_bounds, offset);
                    if matches!(layer, Layer::Clip | Layer::ClipSdf { .. }) {
                        let clip =
                            offset.bounds(pixel_bounds(canvas.draw_records[*draw].pixel_bounds));
                        for node in &mut self.nodes[node_start..] {
                            node.bounds = node.bounds.intersect(clip);
                        }
                    } else if let Layer::Filter {
                        filter: value,
                        sample_region,
                    } = layer
                    {
                        let dependency = offset
                            .bounds(region_bounds(sample_region))
                            .outset(filter::filter_dependency_outset(value));
                        let output = offset
                            .bounds(filter::unclipped_filtered_region_bounds(
                                value,
                                sample_region,
                            ))
                            .intersect(self.canvas_bounds);
                        for node in &mut self.nodes[node_start..] {
                            let changed = node.bounds.intersect(dependency);
                            node.bounds = if changed.is_empty() {
                                empty_bounds()
                            } else {
                                changed
                                    .outset(filter::filter_outset(value))
                                    .intersect(output)
                            };
                        }
                    }
                    if let Some(retained) = retained {
                        self.push_node(
                            retained.id,
                            retained.revision,
                            bounds,
                            RetainedNodeKind::Layer,
                            None,
                        );
                    }
                    bounds
                }
                Command::MaskLayer {
                    retained,
                    layer,
                    content,
                    mask,
                } => {
                    let owns_identity = retained.is_some();
                    if !inside_retained && !owns_identity {
                        self.identity_complete = false;
                    }
                    let node_start = self.nodes.len();
                    let content_bounds =
                        self.visit_list(canvas, *content, inside_retained || owns_identity, offset);
                    self.visit_list(canvas, *mask, inside_retained || owns_identity, offset);
                    let region = offset.bounds(region_bounds(&layer.region));
                    for node in &mut self.nodes[node_start..] {
                        node.bounds = node.bounds.intersect(region);
                    }
                    let bounds = content_bounds.intersect(region);
                    if let Some(retained) = retained {
                        self.push_node(
                            retained.id,
                            retained.revision,
                            bounds,
                            RetainedNodeKind::Layer,
                            None,
                        );
                    }
                    bounds
                }
            };
            bounds = bounds.union(command_bounds);
        }
        bounds
    }
}

fn canvas_visual_bounds(canvas: &Canvas) -> Bounds {
    list_visual_bounds(
        canvas,
        canvas.root_commands,
        SceneOffset { dx: 0.0, dy: 0.0 },
    )
}

fn list_visual_bounds(canvas: &Canvas, list_id: usize, offset: SceneOffset) -> Bounds {
    canvas.command_lists[list_id]
        .commands
        .iter()
        .fold(empty_bounds(), |bounds, command| {
            bounds.union(match command {
                Command::Draw(draw) => {
                    offset.bounds(pixel_bounds(canvas.draw_records[*draw].pixel_bounds))
                }
                Command::RetainedScene {
                    canvas,
                    offset: child,
                    ..
                } => SceneOffset {
                    dx: offset.dx + child.0,
                    dy: offset.dy + child.1,
                }
                .bounds(canvas_visual_bounds(canvas)),
                Command::MaterializedRetainedScene { children, .. } => {
                    list_visual_bounds(canvas, *children, offset)
                }
                Command::Layer {
                    draw,
                    layer,
                    children,
                    ..
                } => layer_bounds(
                    canvas,
                    *draw,
                    layer,
                    list_visual_bounds(canvas, *children, offset),
                    offset,
                ),
                Command::MaskLayer { layer, content, .. } => {
                    list_visual_bounds(canvas, *content, offset)
                        .intersect(offset.bounds(region_bounds(&layer.region)))
                }
            })
        })
}

fn layer_bounds(
    canvas: &Canvas,
    draw: usize,
    layer: &Layer,
    child_bounds: Bounds,
    offset: SceneOffset,
) -> Bounds {
    match layer {
        Layer::Clip
        | Layer::ClipSdf { .. }
        | Layer::Isolate
        | Layer::Opacity(_)
        | Layer::Blend(_) => child_bounds
            .intersect(offset.bounds(pixel_bounds(canvas.draw_records[draw].pixel_bounds))),
        Layer::Filter {
            filter: value,
            sample_region,
        } => offset.bounds(filter::unclipped_filtered_region_bounds(
            value,
            sample_region,
        )),
        Layer::Backdrop {
            filter: value,
            sample_region,
        } => child_bounds.union(offset.bounds(filter::unclipped_filtered_region_bounds(
            value,
            sample_region,
        ))),
    }
}

fn region_bounds(region: &Region) -> Bounds {
    filter::region_bounds(region)
}

fn pixel_bounds(bounds: crate::shared::bounds::PixelBounds) -> Bounds {
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

fn empty_bounds() -> Bounds {
    Bounds::new(0, 0, 0, 0)
}

fn propagate_list_damage(
    canvas: &Canvas,
    list_id: usize,
    pending: &RetainedDamage,
    damage: &mut Vec<Bounds>,
    dirty_backdrops: &mut HashSet<RetainedNodeId>,
    retained_owner: Option<RetainedNodeId>,
) {
    for command in &canvas.command_lists[list_id].commands {
        match command {
            Command::Draw(_) | Command::RetainedScene { .. } => {}
            Command::MaterializedRetainedScene { id, children, .. } => {
                propagate_list_damage(
                    canvas,
                    *children,
                    pending,
                    damage,
                    dirty_backdrops,
                    Some(*id),
                );
                append_node_damage(pending, *id, damage);
            }
            Command::Layer {
                retained,
                layer,
                children,
                ..
            } => {
                let retained_owner = retained.map(|key| key.id).or(retained_owner);
                match layer {
                    Layer::Backdrop {
                        filter: value,
                        sample_region,
                    } => {
                        let source_changed =
                            propagate_filter_damage(canvas, value, sample_region, damage);
                        if source_changed
                            || retained_owner
                                .is_some_and(|id| pending.node_bounds.contains_key(&id))
                        {
                            dirty_backdrops.extend(retained_owner);
                        }
                        // Backdrop children are painted after the sampled background and
                        // therefore cannot invalidate that backdrop in the same frame.
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            damage,
                            dirty_backdrops,
                            retained_owner,
                        );
                    }
                    Layer::Filter {
                        filter: value,
                        sample_region,
                    } => {
                        let mut local = Vec::new();
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            &mut local,
                            dirty_backdrops,
                            retained_owner,
                        );
                        propagate_filter_damage(canvas, value, sample_region, &mut local);
                        append_unique_damage(damage, local);
                    }
                    Layer::Isolate => {
                        let mut local = Vec::new();
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            &mut local,
                            dirty_backdrops,
                            retained_owner,
                        );
                        append_unique_damage(damage, local);
                    }
                    _ => propagate_list_damage(
                        canvas,
                        *children,
                        pending,
                        damage,
                        dirty_backdrops,
                        retained_owner,
                    ),
                }
                if let Some(retained) = retained {
                    append_node_damage(pending, retained.id, damage);
                }
            }
            Command::MaskLayer {
                retained,
                content,
                mask,
                ..
            } => {
                let mut local = Vec::new();
                let retained_owner = retained.map(|key| key.id).or(retained_owner);
                propagate_list_damage(
                    canvas,
                    *content,
                    pending,
                    &mut local,
                    dirty_backdrops,
                    retained_owner,
                );
                propagate_list_damage(
                    canvas,
                    *mask,
                    pending,
                    &mut local,
                    dirty_backdrops,
                    retained_owner,
                );
                // Content and mask damage are both spatial: changing one tile
                // cannot affect a different tile of the masked result.
                append_unique_damage(damage, local);
                if let Some(retained) = retained {
                    append_node_damage(pending, retained.id, damage);
                }
            }
        }
    }
}

fn propagate_filter_damage(
    canvas: &Canvas,
    value: &filter::Filter,
    sample_region: &Region,
    damage: &mut Vec<Bounds>,
) -> bool {
    let dependency =
        filter::region_bounds(sample_region).outset(filter::filter_dependency_outset(value));
    let output = filter::unclipped_filtered_region_bounds(value, sample_region).intersect(
        Bounds::canvas(canvas.physical_width(), canvas.physical_height()),
    );
    let initial_len = damage.len();
    let mut changed = false;
    for index in 0..initial_len {
        let affected = damage[index].intersect(dependency);
        if !affected.is_empty() {
            changed = true;
            push_unique_damage(
                damage,
                affected
                    .outset(filter::filter_outset(value))
                    .intersect(output),
            );
        }
    }
    changed
}

fn append_node_damage(pending: &RetainedDamage, id: RetainedNodeId, damage: &mut Vec<Bounds>) {
    if let Some(bounds) = pending.node_bounds.get(&id) {
        push_unique_damage(damage, *bounds);
    }
}

fn append_unique_damage(damage: &mut Vec<Bounds>, bounds: impl IntoIterator<Item = Bounds>) {
    for bounds in bounds {
        push_unique_damage(damage, bounds);
    }
}

fn push_unique_damage(damage: &mut Vec<Bounds>, bounds: Bounds) {
    if !bounds.is_empty() && !damage.contains(&bounds) {
        damage.push(bounds);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Point, Rect},
    };

    use super::*;
    use crate::{BlurSampling, Filter, Radius, Region};

    fn scene(color: Color) -> Arc<Canvas> {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(Rect::new(2.0, 3.0, 18.0, 20.0), Radius::ZERO, color);
        Arc::new(canvas)
    }

    #[test]
    fn retained_scene_keeps_identity_revision_and_translated_bounds() {
        let root = RetainedNodeId::for_owner(1);
        let child = RetainedNodeId::for_owner(2);
        let mut canvas = Canvas::new_retained(100, 80, 1.0, root);
        canvas.append_retained_scene(
            child,
            7,
            scene(Color::from_rgb8(255, 0, 0)),
            Point::new(10.0, 20.0),
        );

        let frame = canvas.retained_frame().unwrap();
        assert!(frame.incremental_complete);
        assert_eq!(frame.nodes.len(), 1);
        assert_eq!(frame.nodes[0].id, child);
        assert_eq!(frame.nodes[0].revision, SceneRevision::new(7));
        assert_eq!(frame.nodes[0].bounds, Bounds::new(12, 23, 28, 40));
    }

    #[test]
    fn retained_apis_fall_back_to_flat_commands_on_an_immediate_canvas() {
        let mut canvas = Canvas::new(64, 64, 1.0);
        assert!(!canvas.is_retained());
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(2),
            SceneRevision::INITIAL,
            scene(Color::WHITE),
            (4.0, 5.0),
        );
        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(3), SceneRevision::INITIAL),
            Rect::new(0.0, 0.0, 32.0, 32.0),
            Radius::ZERO,
        );
        canvas.pop_layer();

        assert!(!canvas.has_retained_scenes());
        assert!(matches!(
            canvas.command_lists[canvas.root_commands]
                .commands
                .as_slice(),
            [Command::Draw(_), Command::Layer { retained: None, .. }]
        ));
        assert!(canvas.retained_frame().is_none());
    }

    #[test]
    fn cached_scene_is_the_only_retained_node_for_a_drawable() {
        let child = RetainedNodeId::for_owner(3);
        let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
        canvas.append_retained_scene(child, 7, scene(Color::WHITE), (0.0, 0.0));

        assert!(matches!(
            canvas.command_lists[canvas.root_commands]
                .commands
                .as_slice(),
            [Command::RetainedScene { id, .. }] if *id == child
        ));
        assert_eq!(
            canvas.command_lists.len(),
            1,
            "a cached drawable must not create a wrapper command list"
        );
        let frame = canvas.retained_frame().expect("retained frame");
        assert_eq!(
            frame.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![child]
        );
    }

    #[test]
    fn flat_snapshot_adapter_rebuilds_only_changed_child_chunks() {
        let root = RetainedNodeId::for_owner(10);
        let build = |revision, changed: Color| {
            let mut canvas = Canvas::new_retained(64, 64, 1.0, root);
            for index in 0..100 {
                canvas.append_retained_scene(
                    RetainedNodeId::for_owner(index + 11),
                    if index == 0 { revision } else { 0 },
                    scene(if index == 0 { changed } else { Color::WHITE }),
                    ((index % 8) as f64 * 4.0, (index / 8) as f64 * 4.0),
                );
            }
            canvas
        };
        let first = build(0, Color::from_rgb8(220, 30, 40));
        let second = build(1, Color::from_rgb8(30, 210, 70));
        let mut cache = RetainedSceneCache::default();
        let initial = cache.materialize_snapshot_shared(&first);
        assert_eq!(initial.buffer_changes.as_ref().unwrap().chunks_rebuilt, 100);

        let changed = cache.materialize_snapshot_shared(&second);
        let changes = changed.buffer_changes.as_ref().unwrap();
        assert_eq!(changes.chunks_rebuilt, 1);
        assert!(!changes.full_scene_sync);
        assert!(
            changes.cpu_copied_bytes < initial.buffer_changes.as_ref().unwrap().cpu_copied_bytes
        );

        let unchanged = cache.materialize_snapshot_shared(&second);
        assert!(Arc::ptr_eq(&changed, &unchanged));
    }

    #[test]
    fn snapshot_reorder_diff_records_only_the_moved_sibling() {
        let ids = (0..4).map(RetainedNodeId::for_owner).collect::<Vec<_>>();
        assert_eq!(
            snapshot_reorder_moves(&ids, &[ids[1], ids[2], ids[0], ids[3]]),
            vec![(ids[0], ids[3])]
        );
        assert_eq!(
            snapshot_reorder_moves(&ids, &[ids[3], ids[0], ids[1], ids[2]]),
            vec![(ids[3], ids[0])]
        );
    }

    #[test]
    fn retained_scene_cache_trusts_revision_as_content_identity() {
        let id = RetainedNodeId::for_owner(4);
        let revision = SceneRevision::new(7);
        let red = scene(Color::from_rgb8(255, 0, 0));
        let green = scene(Color::from_rgb8(0, 255, 0));
        let mut cache = RetainedSceneCache::default();

        let first = cache.scene(id, revision, &red);
        let reused = cache.scene(id, revision, &green);
        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(cache.scenes.len(), 1);

        let advanced = cache.scene(id, SceneRevision::new(8), &green);
        assert!(Arc::ptr_eq(&advanced, &green));
        assert_eq!(cache.scenes.len(), 1);
        assert_eq!(cache.scenes[&id].revision, SceneRevision::new(8));
    }

    #[test]
    fn retained_layer_owns_direct_commands() {
        let layer = RetainedNodeId::for_owner(2);
        let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(layer, SceneRevision::INITIAL),
            Rect::new(0.0, 0.0, 32.0, 32.0),
            Radius::ZERO,
        );
        canvas.push_rect(Rect::new(4.0, 4.0, 20.0, 20.0), Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        let frame = canvas.retained_frame().expect("retained frame");
        assert!(frame.incremental_complete);
        assert_eq!(frame.nodes.len(), 1);
        assert_eq!(frame.nodes[0].id, layer);
        assert_eq!(frame.nodes[0].kind, RetainedNodeKind::Layer);
        assert_eq!(frame.nodes[0].revision, SceneRevision::INITIAL);
    }

    #[test]
    fn retained_layer_keeps_identity_for_layer_semantics() {
        let layer = RetainedNodeId::for_owner(2);
        let child = RetainedNodeId::for_owner(3);
        let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(layer, SceneRevision::INITIAL),
            Rect::new(0.0, 0.0, 32.0, 32.0),
            Radius::ZERO,
        );
        canvas.append_retained_scene(child, 0, scene(Color::WHITE), (0.0, 0.0));
        canvas.pop_layer();

        let frame = canvas.retained_frame().expect("retained frame");
        assert_eq!(
            frame.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            vec![child, layer]
        );
        assert_eq!(frame.nodes[1].revision, SceneRevision::INITIAL);
    }

    #[test]
    fn retained_layer_revision_is_the_explicit_content_identity() {
        let build = |color, revision| {
            let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
            canvas.push_retained_clip_sdf_rect_layer(
                RetainedLayerKey::new(RetainedNodeId::for_owner(2), revision),
                Rect::new(0.0, 0.0, 32.0, 32.0),
                Radius::ZERO,
            );
            canvas.push_rect(Rect::new(4.0, 4.0, 20.0, 20.0), Radius::ZERO, color);
            canvas.pop_layer();
            canvas.retained_frame().expect("retained frame")
        };

        let red = build(Color::from_rgb8(255, 0, 0), SceneRevision::INITIAL);
        let stale_green = build(Color::from_rgb8(0, 255, 0), SceneRevision::INITIAL);
        let green = build(Color::from_rgb8(0, 255, 0), SceneRevision::new(1));

        assert_eq!(red.nodes, stale_green.nodes);
        assert_ne!(red.nodes, green.nodes);
    }

    #[test]
    fn manual_damage_is_scaled_and_clipped() {
        let mut canvas = Canvas::new_retained(20, 10, 2.0, RetainedNodeId::for_owner(1));
        canvas.invalidate_rect(Rect::new(-2.0, 1.25, 4.25, 12.0));
        assert_eq!(
            canvas.retained_frame().unwrap().invalidated_bounds,
            vec![Bounds::new(0, 2, 9, 20)]
        );
    }

    #[test]
    fn manual_damage_does_not_make_untracked_commands_cacheable() {
        let mut canvas = Canvas::new_retained(32, 16, 1.0, RetainedNodeId::for_owner(1));
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::WHITE);
        canvas.invalidate_rect(Rect::new(0.0, 0.0, 16.0, 16.0));

        let frame = canvas.retained_frame().expect("retained frame");
        assert!(frame.incremental_complete);
        assert!(!frame.materialization_cacheable);
    }

    #[test]
    fn retained_child_influence_is_clipped_by_ancestor_layer() {
        let mut child = Canvas::new(64, 16, 1.0);
        child.push_rect(Rect::new(0.0, 0.0, 64.0, 16.0), Radius::ZERO, Color::WHITE);
        let child_id = RetainedNodeId::for_owner(3);
        let mut canvas = Canvas::new_retained(64, 16, 1.0, RetainedNodeId::for_owner(1));
        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(2), SceneRevision::INITIAL),
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Radius::ZERO,
        );
        canvas.append_retained_scene(child_id, 0, Arc::new(child), (0.0, 0.0));
        canvas.pop_layer();

        let frame = canvas.retained_frame().expect("retained frame");
        assert_eq!(
            frame
                .nodes
                .iter()
                .find(|node| node.id == child_id)
                .expect("child node")
                .bounds,
            Bounds::new(0, 0, 16, 16)
        );
    }

    #[test]
    fn filter_maps_off_canvas_source_changes_back_into_visible_bounds() {
        let child_id = RetainedNodeId::for_owner(3);
        let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
        canvas.push_retained_filter_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(2), SceneRevision::INITIAL),
            Filter::Blur {
                std_dev_x: 4.0,
                std_dev_y: 4.0,
                sampling: BlurSampling::default(),
            },
            Region::rect(Rect::new(0.0, 0.0, 80.0, 80.0), Radius::ZERO),
        );
        canvas.append_retained_scene(child_id, 0, scene(Color::WHITE), (40.0, 52.0));
        canvas.pop_layer();

        let frame = canvas.retained_frame().expect("retained frame");
        let child = frame
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .expect("child node");
        assert_eq!(child.bounds.y1, 64);
        assert!(child.bounds.x0 < 42 && child.bounds.y0 < 55);
    }
}
