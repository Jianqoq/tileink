use std::{collections::HashSet, sync::Arc};

use peniko::kurbo::Rect;
use rustc_hash::FxHashMap as HashMap;

use super::{Canvas, SceneOffset};
use crate::shared::{
    bounds::Bounds,
    execution::Command,
    layer::{Layer, filter, region::Region},
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

/// Internal generation attached to persistent materialized commands and surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct NodeGeneration(u64);

impl NodeGeneration {
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
    pub(crate) incremental_complete: bool,
    /// Persistent-scene version and immutable journal overlay.
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

    pub(crate) fn node_revision(&self, id: RetainedNodeId) -> Option<NodeGeneration> {
        self.node_state(id).map(|node| node.revision)
    }
}

impl Canvas {
    /// Applies journal damage to the internal materialized canvas.
    pub(crate) fn invalidate_rect(&mut self, rect: Rect) {
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

    pub(crate) fn retained_frame(&self) -> Option<RetainedFrame> {
        self.persistent_frame.clone()
    }

    pub(crate) fn visual_bounds(&self) -> Bounds {
        canvas_visual_bounds(self)
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
            Command::Draw(_) => {}
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
