use std::rc::Rc;

#[cfg(test)]
use std::collections::HashMap;

use crate::{
    Canvas, TILE_SIZE,
    canvas::{RetainedDamage, RetainedFrame},
    shared::{
        bounds::Bounds,
        gpu_plan::{CUMSUM_CHUNK_SIZE, GpuCumsumPlan},
    },
};

use super::damage_tiles::DamageTiles;

#[cfg(feature = "bench-internals")]
mod benchmark;
#[cfg(feature = "bench-internals")]
pub use benchmark::{FrameDiffBenchmark, FrameDiffBenchmarkCase};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncrementalRenderMode {
    #[default]
    Auto,
    ForceFull,
}

/// Native coarse-kernel policy for incremental frames.
///
/// Forced modes support profiling and controlled deployments. Full redraws always use dense bins
/// because compact dispatch requires an active-tile worklist.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoarseBinningMode {
    #[default]
    Auto,
    ForceCompact,
    ForceDense,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncrementalRenderConfig {
    pub mode: IncrementalRenderMode,
    pub coarse_binning: CoarseBinningMode,
    /// Dirty-tile ratio that selects the full fast path and transient direct output.
    pub full_redraw_ratio: f32,
    /// Ratio below which transient direct rendering starts its history-rebuild countdown.
    pub direct_render_exit_ratio: f32,
    /// Consecutive low-damage frames required before rebuilding renderer-owned history.
    pub direct_render_exit_frames: u32,
    /// Capture exact active tile IDs and regions in [`IncrementalRenderStats`].
    ///
    /// Disabled by default so ordinary rendering does not allocate diagnostic vectors per frame.
    /// The WGPU profiler captures them automatically for profiled frames regardless of this flag.
    pub capture_active_tiles: bool,
    pub retained_texture_budget_bytes: u64,
}

impl Default for IncrementalRenderConfig {
    fn default() -> Self {
        Self {
            mode: IncrementalRenderMode::Auto,
            coarse_binning: CoarseBinningMode::Auto,
            full_redraw_ratio: 0.7,
            direct_render_exit_ratio: 0.4,
            direct_render_exit_frames: 2,
            capture_active_tiles: false,
            retained_texture_budget_bytes: if cfg!(target_arch = "wasm32") {
                64 * 1024 * 1024
            } else {
                256 * 1024 * 1024
            },
        }
    }
}

impl IncrementalRenderConfig {
    pub fn validate(self) -> Self {
        assert!(
            self.full_redraw_ratio.is_finite()
                && self.full_redraw_ratio > 0.0
                && self.full_redraw_ratio <= 1.0,
            "full_redraw_ratio must be in (0, 1]"
        );
        assert!(
            self.direct_render_exit_ratio.is_finite()
                && self.direct_render_exit_ratio >= 0.0
                && self.direct_render_exit_ratio < self.full_redraw_ratio,
            "direct_render_exit_ratio must be in [0, full_redraw_ratio)"
        );
        assert!(
            self.direct_render_exit_frames > 0,
            "direct_render_exit_frames must be greater than zero"
        );
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullRedrawReason {
    Forced,
    NonRetainedCanvas,
    FirstFrame,
    RootChanged,
    SurfaceChanged,
    UntrackedContent,
    ExplicitInvalidation,
    RendererStateChanged,
    DirtyTileThreshold,
}

/// Destination strategy used for the most recent retained frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncrementalOutputMode {
    /// Renderer-owned history was updated. A texture render may copy it to the caller afterward.
    #[default]
    InternalHistory,
    /// A high-damage frame was rendered straight to a transient caller texture.
    DirectTransient,
    /// Renderer-owned history was rebuilt after transient direct rendering ended.
    RebuildHistory,
    /// A caller-owned persistent texture is itself the retained history.
    ExternalHistory,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncrementalRenderStats {
    pub full_redraw: bool,
    pub full_redraw_reason: Option<FullRedrawReason>,
    pub dirty_tiles: u32,
    pub total_tiles: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub dirty_ratio: f32,
    /// Scene damage before a missing internal history forces full rendering.
    pub changed_tiles: u32,
    pub changed_ratio: f32,
    pub output_mode: IncrementalOutputMode,
    pub history_copied_to_output: bool,
    /// Queue submissions issued by the renderer for the frame, excluding diagnostic readbacks.
    pub queue_submissions: u32,
    /// Exact active tile IDs captured for profiler/diagnostic frames.
    pub active_tiles: Vec<u32>,
    /// Coalesced pixel-space regions for [`Self::active_tiles`].
    pub active_tile_bounds: Vec<Bounds>,
    pub retained_nodes: u32,
    /// Coarse/fine batches encoded across root and offscreen targets.
    pub draw_batches: u32,
    /// Incremental batches whose coarse stage used dense 16x16-tile bins because its estimated
    /// dispatch and serial candidate-loop cost was lower than compact tile-parallel execution.
    pub dense_coarse_batches: u32,
    /// Coarse/fine batches that write the root target.
    pub root_draw_batches: u32,
    pub reused_offscreen_surfaces: u32,
    pub rerendered_offscreen_surfaces: u32,
    pub rerendered_offscreen_tiles: u32,
    /// Retained nodes visited to reconcile offscreen-surface ownership. Static frames and
    /// journal-connected commits keep this at zero.
    pub retained_surface_nodes_scanned: u32,
    pub scanned_paths: u32,
    pub scanned_lines: u32,
    pub scan_chunks: u32,
    /// Total filter compute passes encoded for this frame.
    pub filter_dispatches: u32,
    /// Filter passes encoded as one workgroup per active dirty tile.
    pub compact_filter_dispatches: u32,
    /// Whether the renderer reused the already-flattened retained scene.
    pub materialized_scene_reused: bool,
    pub reused_compiled_plan: bool,
    /// Persistent chunks encoded for this frame.
    pub chunks_rebuilt: u32,
    /// Persistent execution-plan fragments patched or rebuilt this frame.
    pub plan_fragments_rebuilt: u32,
    /// Whether journal loss, surface change, or first use required a complete scene resync.
    pub full_scene_sync: bool,
    /// Bytes copied from changed chunk allocations into stable CPU scene arrays.
    pub cpu_copied_bytes: u64,
    /// Bytes written to scene storage buffers, excluding render targets and transient worklists.
    pub gpu_uploaded_bytes: u64,
    pub tile_pages_rewritten: u32,
    pub tile_page_compactions: u64,
    pub arena_live_bytes: u64,
    pub arena_capacity_bytes: u64,
    pub arena_fragmentation: f32,
    pub arena_compactions: u64,
}

/// Compact scan/cumsum work derived from the paths that can affect active tiles.
///
/// Every selected path is scanned over its complete geometry and backdrop
/// allocation. This is conservative enough for winding and clipping while
/// avoiding work for paths whose influence bounds cannot reach a dirty tile.
pub(crate) struct ActiveScanPlan {
    pub(crate) indices: Vec<u32>,
    pub(crate) line_base: u32,
    pub(crate) line_count: u32,
    pub(crate) path_base: u32,
    pub(crate) path_count: u32,
    pub(crate) chunk_base: u32,
    pub(crate) chunk_count: u32,
    pub(crate) backdrop_base: u32,
    pub(crate) backdrop_count: u32,
    pub(crate) cumsum: GpuCumsumPlan,
}

impl ActiveScanPlan {
    pub(crate) fn new(
        canvas: &Canvas,
        damage: &DamageTiles,
        scan_ranges: &[crate::shared::gpu_plan::GpuScanChunkRange],
    ) -> Self {
        let mut lines = Vec::new();
        let mut paths = Vec::new();
        let mut chunks = Vec::new();
        let mut backdrops = Vec::new();
        let mut cumsum = GpuCumsumPlan::default();
        for (path_index, record) in canvas.path_records.iter().enumerate() {
            // Retained path slots can contain holes, and their scan chunks live in a
            // persistent variable-sized arena. Deriving chunk indices from current
            // record order would address the wrong allocation after removals or growth.
            if !record.is_live_at(path_index) {
                continue;
            }
            let active = damage.intersects_tile_rect(
                record.tile_x0,
                record.tile_y0,
                record.tile_x1,
                record.tile_y1,
            );
            if active {
                paths.push(record.path_id);
                lines
                    .extend(record.line_start..record.line_start.saturating_add(record.line_count));
                let range = scan_ranges
                    .get(path_index)
                    .expect("persistent scan range exists for every path slot");
                chunks.extend(range.start..range.end);
                backdrops
                    .extend(record.data_offset..record.data_offset.saturating_add(record.data_len));
                append_cumsum_path(record, &mut cumsum);
            }
        }

        let line_base = 0;
        let line_count = lines.len() as u32;
        let path_base = line_count;
        let path_count = paths.len() as u32;
        let chunk_base = path_base + path_count;
        let chunk_count = chunks.len() as u32;
        let backdrop_base = chunk_base + chunk_count;
        let backdrop_count = backdrops.len() as u32;
        let mut indices =
            Vec::with_capacity(lines.len() + paths.len() + chunks.len() + backdrops.len());
        indices.extend(lines);
        indices.extend(paths);
        indices.extend(chunks);
        indices.extend(backdrops);
        Self {
            indices,
            line_base,
            line_count,
            path_base,
            path_count,
            chunk_base,
            chunk_count,
            backdrop_base,
            backdrop_count,
            cumsum,
        }
    }
}

fn append_cumsum_path(record: &crate::shared::path::PathRecord, plan: &mut GpuCumsumPlan) {
    let stride = record.tile_x1.saturating_sub(record.tile_x0);
    let height = record.tile_y1.saturating_sub(record.tile_y0);
    for row in 0..height {
        let row_start = plan.chunk_backdrop_offsets.len() as u32;
        let row_offset = record.data_offset + row * stride;
        let mut local_x = 0;
        while local_x < stride {
            let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
            plan.chunk_backdrop_offsets.push(row_offset + local_x);
            plan.chunk_lens.push(len);
            local_x += len;
        }
        plan.row_chunk_starts.push(row_start);
        plan.row_chunk_ends
            .push(plan.chunk_backdrop_offsets.len() as u32);
    }
}

pub(crate) struct DamagePlan {
    pub(crate) tiles: DamageTiles,
    pub(crate) changed_tiles: DamageTiles,
    pub(crate) retained_damage: RetainedDamage,
    pub(crate) stats: IncrementalRenderStats,
    pub(crate) frame: Option<RetainedFrame>,
    /// `Some` means the consumed persistent delta already expanded backdrop output damage and
    /// identified every retained backdrop surface that must be rerendered.
    pub(crate) dirty_backdrops: Option<std::rc::Rc<[crate::RetainedNodeId]>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransientOutputDecision {
    InternalHistory,
    Direct,
    RebuildHistory,
}

/// Tracks whether renderer-owned history is intentionally stale while full frames are sent
/// straight to transient output textures.
#[derive(Default)]
pub(crate) struct TransientOutputState {
    active: bool,
    low_damage_frames: u32,
}

impl TransientOutputState {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn decide(
        &mut self,
        stats: &IncrementalRenderStats,
        config: IncrementalRenderConfig,
    ) -> TransientOutputDecision {
        if config.mode == IncrementalRenderMode::ForceFull {
            self.reset();
            return TransientOutputDecision::InternalHistory;
        }

        if !self.active {
            let initial_history_build =
                stats.full_redraw_reason == Some(FullRedrawReason::FirstFrame);
            if !initial_history_build && stats.changed_ratio >= config.full_redraw_ratio {
                self.active = true;
                self.low_damage_frames = 0;
                return TransientOutputDecision::Direct;
            }
            return TransientOutputDecision::InternalHistory;
        }

        if stats.changed_ratio <= config.direct_render_exit_ratio {
            self.low_damage_frames = self.low_damage_frames.saturating_add(1);
            if self.low_damage_frames >= config.direct_render_exit_frames {
                self.reset();
                return TransientOutputDecision::RebuildHistory;
            }
        } else {
            self.low_damage_frames = 0;
        }
        TransientOutputDecision::Direct
    }
}

impl DamagePlan {
    pub(crate) fn include_dependent_bounds(
        &mut self,
        bounds: impl IntoIterator<Item = Bounds>,
        config: IncrementalRenderConfig,
    ) {
        for bounds in bounds {
            self.changed_tiles.add_bounds(bounds);
        }
        self.stats.changed_tiles = self.changed_tiles.len();
        self.stats.changed_ratio = tile_ratio(&self.changed_tiles);
        if !self.stats.full_redraw && self.stats.changed_ratio >= config.full_redraw_ratio {
            let (tiles_width, tiles_height) = self.changed_tiles.dimensions();
            let size = (tiles_width * TILE_SIZE, tiles_height * TILE_SIZE);
            self.tiles = DamageTiles::full(size);
            self.stats.full_redraw = true;
            self.stats.full_redraw_reason = Some(FullRedrawReason::DirtyTileThreshold);
        } else if !self.stats.full_redraw {
            self.tiles = self.changed_tiles.clone();
        }
        self.stats.dirty_tiles = self.tiles.len();
        self.stats.dirty_ratio = tile_ratio(&self.tiles);
    }
}

fn tile_ratio(tiles: &DamageTiles) -> f32 {
    if tiles.total_tiles() == 0 {
        0.0
    } else {
        tiles.len() as f32 / tiles.total_tiles() as f32
    }
}

#[derive(Default)]
pub(crate) struct IncrementalState {
    previous: Option<RetainedFrame>,
    renderer_state_invalid: bool,
    frame_diff: FrameDiffScratch,
}

#[derive(Clone, Copy)]
struct CommonNode {
    new_index: usize,
    influence: Bounds,
}

/// Reusable CPU storage for retained-frame comparison.
///
/// Frame indexes already live in `RetainedFrame`; this scratch only stores the common-node order
/// and a flat tile adjacency while checking painter-order inversions. Keeping it on the renderer
/// avoids rebuilding hash collections and thousands of per-tile vectors every frame.
#[derive(Default)]
struct FrameDiffScratch {
    common: Vec<CommonNode>,
    tile_offsets: Vec<usize>,
    tile_cursors: Vec<usize>,
    tile_nodes: Vec<usize>,
    previous_overrides:
        rustc_hash::FxHashMap<crate::RetainedNodeId, Option<crate::canvas::RetainedNodeState>>,
    current_overrides:
        rustc_hash::FxHashMap<crate::RetainedNodeId, Option<crate::canvas::RetainedNodeState>>,
    previous_affected: Vec<u8>,
    current_affected: Vec<u8>,
}

impl IncrementalState {
    pub(crate) fn invalidate_renderer_state(&mut self) {
        self.renderer_state_invalid = true;
    }

    pub(crate) fn plan(
        &mut self,
        frame: Option<RetainedFrame>,
        physical_size: (u32, u32),
        config: IncrementalRenderConfig,
        history_valid: bool,
    ) -> DamagePlan {
        let size = physical_size;
        let mut stats = IncrementalRenderStats {
            total_tiles: size.0.div_ceil(TILE_SIZE) * size.1.div_ceil(TILE_SIZE),
            tiles_width: size.0.div_ceil(TILE_SIZE),
            tiles_height: size.1.div_ceil(TILE_SIZE),
            retained_nodes: frame.as_ref().map_or(0, |frame| frame.nodes.len() as u32),
            ..Default::default()
        };

        let reason = self.full_reason(frame.as_ref(), config.mode, history_valid);
        let dirty_backdrops = frame.as_ref().and_then(|current| {
            let previous_version = self.previous.as_ref()?.version?;
            let delta = current.delta.as_ref()?;
            (delta.backdrop_damage_complete
                && delta.from_version == previous_version
                && current.version == Some(delta.to_version)
                && !current.invalidate_all)
                .then(|| delta.dirty_backdrops.clone())
        });
        let (changed_tiles, retained_damage) = self.scene_damage(frame.as_ref(), size);
        let threshold_exceeded =
            reason.is_none() && tile_ratio(&changed_tiles) >= config.full_redraw_ratio;
        let reason = if threshold_exceeded {
            Some(FullRedrawReason::DirtyTileThreshold)
        } else {
            reason
        };
        let tiles = if reason.is_some() {
            DamageTiles::full(size)
        } else {
            changed_tiles.clone()
        };

        stats.full_redraw = reason.is_some();
        stats.full_redraw_reason = reason;
        stats.dirty_tiles = tiles.len();
        stats.dirty_ratio = tile_ratio(&tiles);
        stats.changed_tiles = changed_tiles.len();
        stats.changed_ratio = tile_ratio(&changed_tiles);
        DamagePlan {
            tiles,
            changed_tiles,
            retained_damage,
            stats,
            frame,
            dirty_backdrops,
        }
    }

    fn scene_damage(
        &mut self,
        frame: Option<&RetainedFrame>,
        physical_size: (u32, u32),
    ) -> (DamageTiles, RetainedDamage) {
        let Some(current) = frame else {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        };
        let Some(previous) = self.previous.as_ref() else {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        };
        if self.renderer_state_invalid
            || previous.root != current.root
            || previous.logical_size != current.logical_size
            || previous.physical_size != current.physical_size
            || previous.scale_bits != current.scale_bits
            || !previous.incremental_complete
            || !current.incremental_complete
            || current.invalidate_all
        {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        }

        if let (Some(previous_version), Some(delta)) = (previous.version, &current.delta)
            && delta.from_version == previous_version
            && current.version == Some(delta.to_version)
        {
            let mut tiles = DamageTiles::new(physical_size);
            let mut retained = RetainedDamage::default();
            for patch in delta.patches.iter() {
                if patch.old != patch.new {
                    for region in patch.damage_regions() {
                        tiles.add_bounds(region);
                    }
                    let Some(bounds) = patch.damage_bounds() else {
                        continue;
                    };
                    if let Some(node) = patch.new.or(patch.old) {
                        retained.add_node(node.id, bounds);
                    }
                }
            }
            for &(id, bounds) in delta.damage.iter() {
                tiles.add_bounds(bounds);
                retained.add_node(id, bounds);
            }
            for &bounds in &current.invalidated_bounds {
                tiles.add_bounds(bounds);
                retained.add_unattributed(bounds);
            }
            return (tiles, retained);
        }

        // A persistent scene version is collected once and shared by subsequent static frames.
        // Pointer identity makes static and raster-only invalidation independent of node count.
        if Rc::ptr_eq(&previous.nodes, &current.nodes) {
            let mut tiles = DamageTiles::new(physical_size);
            let mut retained = RetainedDamage::default();
            for &bounds in &current.invalidated_bounds {
                tiles.add_bounds(bounds);
                retained.add_unattributed(bounds);
            }
            return (tiles, retained);
        }

        let mut tiles = DamageTiles::new(physical_size);
        let mut retained = RetainedDamage::default();
        diff_frames_reusing(
            previous,
            current,
            &mut tiles,
            &mut retained,
            &mut self.frame_diff,
        );
        for bounds in &current.invalidated_bounds {
            tiles.add_bounds(*bounds);
            retained.add_unattributed(*bounds);
        }
        (tiles, retained)
    }

    pub(crate) fn commit(&mut self, frame: Option<RetainedFrame>) {
        self.previous = frame;
        self.renderer_state_invalid = false;
    }

    fn full_reason(
        &self,
        frame: Option<&RetainedFrame>,
        mode: IncrementalRenderMode,
        history_valid: bool,
    ) -> Option<FullRedrawReason> {
        if mode == IncrementalRenderMode::ForceFull {
            return Some(FullRedrawReason::Forced);
        }
        let Some(frame) = frame else {
            return Some(FullRedrawReason::NonRetainedCanvas);
        };
        if self.renderer_state_invalid && self.previous.is_some() {
            return Some(FullRedrawReason::RendererStateChanged);
        }
        if self.previous.is_none() {
            return Some(FullRedrawReason::FirstFrame);
        }
        let previous = self.previous.as_ref().unwrap();
        if previous.root != frame.root {
            return Some(FullRedrawReason::RootChanged);
        }
        if previous.logical_size != frame.logical_size
            || previous.physical_size != frame.physical_size
            || previous.scale_bits != frame.scale_bits
        {
            return Some(FullRedrawReason::SurfaceChanged);
        }
        // An incomplete frame may contain direct commands that are absent from
        // the retained-node diff. It is safe to draw such a frame in full, but
        // it must never become the baseline for a later incremental frame: a
        // removed untracked command would otherwise leave its old pixels in
        // the history texture.
        if !previous.incremental_complete || !frame.incremental_complete {
            return Some(FullRedrawReason::UntrackedContent);
        }
        if frame.invalidate_all {
            return Some(FullRedrawReason::ExplicitInvalidation);
        }
        if !history_valid {
            return Some(FullRedrawReason::FirstFrame);
        }
        None
    }
}

fn diff_frames_reusing(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    scratch.common.clear();
    if previous.delta.is_none()
        && current.delta.is_none()
        && previous.state_pages.is_empty()
        && current.state_pages.is_empty()
    {
        diff_base_nodes(previous, current, damage, retained, &mut scratch.common);
    } else {
        collect_logical_overrides(previous, &mut scratch.previous_overrides);
        collect_logical_overrides(current, &mut scratch.current_overrides);
        if scratch.previous_overrides.is_empty() && scratch.current_overrides.is_empty() {
            diff_base_nodes(previous, current, damage, retained, &mut scratch.common);
        } else {
            mark_affected_base_nodes(previous, current, scratch);
            diff_unaffected_base_nodes(previous, current, damage, retained, scratch);
            damage_overridden_nodes(previous, current, damage, retained, scratch);
        }
    }

    let mut delta = current.delta.as_deref();
    while let Some(current) = delta {
        for &(id, bounds) in current.damage.iter() {
            damage.add_bounds(bounds);
            retained.add_node(id, bounds);
        }
        delta = current.previous.as_deref();
    }

    if !scratch
        .common
        .windows(2)
        .all(|pair| pair[0].new_index < pair[1].new_index)
    {
        damage_reordered_nodes(damage, retained, scratch);
    }
}

fn diff_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    common: &mut Vec<CommonNode>,
) {
    for node in current.nodes.iter() {
        let Some(&previous_index) = previous.node_index.get(&node.id) else {
            add_state_change_damage(damage, retained, None, Some(*node));
            continue;
        };
        let old = previous.nodes[previous_index];
        if node_output_changed(old, *node) {
            add_state_change_damage(damage, retained, Some(old), Some(*node));
        }
    }
    for node in previous.nodes.iter() {
        if let Some(&new_index) = current.node_index.get(&node.id) {
            common.push(CommonNode {
                new_index,
                influence: node.bounds.union(current.nodes[new_index].bounds),
            });
        } else {
            add_state_change_damage(damage, retained, Some(*node), None);
        }
    }
}

fn mark_affected_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    scratch: &mut FrameDiffScratch,
) {
    scratch.previous_affected.resize(previous.nodes.len(), 0);
    scratch.previous_affected.fill(0);
    scratch.current_affected.resize(current.nodes.len(), 0);
    scratch.current_affected.fill(0);
    for &id in scratch
        .previous_overrides
        .keys()
        .chain(scratch.current_overrides.keys())
    {
        if let Some(&index) = previous.node_index.get(&id) {
            scratch.previous_affected[index] = 1;
        }
        if let Some(&index) = current.node_index.get(&id) {
            scratch.current_affected[index] = 1;
        }
    }
}

fn diff_unaffected_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    for (index, node) in current.nodes.iter().enumerate() {
        if scratch.current_affected[index] != 0 {
            continue;
        }
        let Some(&previous_index) = previous.node_index.get(&node.id) else {
            add_state_change_damage(damage, retained, None, Some(*node));
            continue;
        };
        let old = previous.nodes[previous_index];
        if node_output_changed(old, *node) {
            add_state_change_damage(damage, retained, Some(old), Some(*node));
        }
    }
    for (index, node) in previous.nodes.iter().enumerate() {
        if scratch.previous_affected[index] != 0 {
            let Some(old) = resolved_node_state(previous, &scratch.previous_overrides, node.id)
            else {
                continue;
            };
            if let Some(&new_index) = current.node_index.get(&node.id)
                && let Some(new) = resolved_node_state(current, &scratch.current_overrides, node.id)
            {
                scratch.common.push(CommonNode {
                    new_index,
                    influence: old.bounds.union(new.bounds),
                });
            }
            continue;
        }
        if let Some(&new_index) = current.node_index.get(&node.id) {
            scratch.common.push(CommonNode {
                new_index,
                influence: node.bounds.union(current.nodes[new_index].bounds),
            });
        } else {
            add_state_change_damage(damage, retained, Some(*node), None);
        }
    }
}

fn node_output_changed(
    old: crate::canvas::RetainedNodeState,
    new: crate::canvas::RetainedNodeState,
) -> bool {
    old.revision != new.revision
        || old.kind != new.kind
        || old.placement_bits != new.placement_bits
        || old.bounds != new.bounds
}

fn collect_logical_overrides(
    frame: &RetainedFrame,
    overrides: &mut rustc_hash::FxHashMap<
        crate::RetainedNodeId,
        Option<crate::canvas::RetainedNodeState>,
    >,
) {
    overrides.clear();
    let mut delta = frame.delta.as_deref();
    while let Some(current) = delta {
        for patch in current.patches.iter() {
            let id = patch.new.or(patch.old).unwrap().id;
            overrides.entry(id).or_insert(patch.new);
        }
        delta = current.previous.as_deref();
    }
    const PAGE_SIZE: usize = 256;
    for (&page_index, page) in frame.state_pages.iter() {
        let start = page_index * PAGE_SIZE;
        for (offset, &state) in page.iter().enumerate() {
            let base = frame.nodes[start + offset];
            if state != base {
                overrides.entry(state.id).or_insert(Some(state));
            }
        }
    }
}

fn resolved_node_state(
    frame: &RetainedFrame,
    overrides: &rustc_hash::FxHashMap<
        crate::RetainedNodeId,
        Option<crate::canvas::RetainedNodeState>,
    >,
    id: crate::RetainedNodeId,
) -> Option<crate::canvas::RetainedNodeState> {
    overrides
        .get(&id)
        .copied()
        .unwrap_or_else(|| frame.node_index.get(&id).map(|&index| frame.nodes[index]))
}

fn damage_overridden_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &FrameDiffScratch,
) {
    for &id in scratch.current_overrides.keys().chain(
        scratch
            .previous_overrides
            .keys()
            .filter(|id| !scratch.current_overrides.contains_key(id)),
    ) {
        let old = resolved_node_state(previous, &scratch.previous_overrides, id);
        let new = resolved_node_state(current, &scratch.current_overrides, id);
        if old.is_none_or(|old| new.is_none_or(|new| node_output_changed(old, new))) {
            add_state_change_damage(damage, retained, old, new);
        } else if let Some(state) = new
            && (!previous.node_index.contains_key(&id) || !current.node_index.contains_key(&id))
        {
            // Overlay-only nodes have no stable base painter index. Redrawing their local
            // influence is the conservative equivalent of including them in the order pass.
            damage.add_bounds(state.bounds);
            retained.add_node(id, state.bounds);
        }
    }
}

fn add_state_change_damage(
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    old: Option<crate::canvas::RetainedNodeState>,
    new: Option<crate::canvas::RetainedNodeState>,
) {
    let bounds = match (old, new) {
        (Some(old), Some(new)) => old.bounds.union(new.bounds),
        (Some(old), None) => {
            damage.add_bounds(old.bounds);
            // Removed commands have no insertion point in the current painter order, so expose
            // their old pixels to every backdrop dependency.
            retained.add_unattributed(old.bounds);
            return;
        }
        (None, Some(new)) => new.bounds,
        (None, None) => return,
    };
    damage.add_bounds(bounds);
    retained.add_node(new.or(old).unwrap().id, bounds);
}

#[cfg(test)]
fn diff_frames(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
) {
    diff_frames_reusing(
        previous,
        current,
        damage,
        retained,
        &mut FrameDiffScratch::default(),
    );
}

/// Damages overlapping nodes whose painter order was inverted.
///
/// Reordering previously compared every common node pair, making even a reorder of disjoint
/// components quadratic. Tile buckets restrict comparisons to spatial neighbors. The remaining
/// worst case is intentionally output-sensitive: mutually overlapping reordered nodes can have
/// O(n²) real inversions, but processing stops as soon as every output tile is already dirty.
fn damage_reordered_nodes(
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    let (tiles_width, tiles_height) = damage.dimensions();
    let tile_count = damage.total_tiles() as usize;
    scratch.tile_offsets.clear();
    scratch.tile_offsets.resize(tile_count + 1, 0);

    for node in &scratch.common {
        if let Some(rect) = tile_rect(node.influence, tiles_width, tiles_height) {
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    scratch.tile_offsets[(y * tiles_width + x) as usize + 1] += 1;
                }
            }
        }
    }
    for tile in 0..tile_count {
        scratch.tile_offsets[tile + 1] += scratch.tile_offsets[tile];
    }

    scratch.tile_cursors.clear();
    scratch
        .tile_cursors
        .extend_from_slice(&scratch.tile_offsets[..tile_count]);
    scratch.tile_nodes.clear();
    scratch
        .tile_nodes
        .resize(scratch.tile_offsets[tile_count], 0);
    for (node_index, node) in scratch.common.iter().enumerate() {
        if let Some(rect) = tile_rect(node.influence, tiles_width, tiles_height) {
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let tile = (y * tiles_width + x) as usize;
                    let cursor = &mut scratch.tile_cursors[tile];
                    scratch.tile_nodes[*cursor] = node_index;
                    *cursor += 1;
                }
            }
        }
    }

    for tile in 0..tile_count {
        let bucket =
            &scratch.tile_nodes[scratch.tile_offsets[tile]..scratch.tile_offsets[tile + 1]];
        if bucket
            .windows(2)
            .all(|pair| scratch.common[pair[0]].new_index < scratch.common[pair[1]].new_index)
        {
            continue;
        }
        for (offset, &a) in bucket.iter().enumerate() {
            for &b in &bucket[offset + 1..] {
                if scratch.common[a].new_index < scratch.common[b].new_index {
                    continue;
                }
                let bounds = scratch.common[a]
                    .influence
                    .intersect(scratch.common[b].influence);
                let Some(rect) = tile_rect(bounds, tiles_width, tiles_height) else {
                    continue;
                };
                // A pair can share many tile buckets. Its intersection's top-left tile is shared
                // by both nodes and gives it one allocation-free canonical comparison site.
                if tile != (rect.y0 * tiles_width + rect.x0) as usize {
                    continue;
                }
                damage.add_bounds(bounds);
                retained.add_unattributed(bounds);
                if damage.len() == damage.total_tiles() {
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TileRect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[inline]
fn tile_rect(bounds: Bounds, tiles_width: u32, tiles_height: u32) -> Option<TileRect> {
    let bounds = bounds.intersect(Bounds::canvas(
        tiles_width.saturating_mul(TILE_SIZE),
        tiles_height.saturating_mul(TILE_SIZE),
    ));
    if bounds.is_empty() {
        return None;
    }
    Some(TileRect {
        x0: bounds.x0.max(0) as u32 / TILE_SIZE,
        y0: bounds.y0.max(0) as u32 / TILE_SIZE,
        x1: (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(tiles_width),
        y1: (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(tiles_height),
    })
}

#[cfg(test)]
mod tests {
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
            dependency_free: false,
            requires_damage_propagation: true,
        }
    }

    #[test]
    fn revision_change_dirties_union_of_old_and_new_tiles() {
        let old = frame(&[(2, 0, Bounds::new(1, 1, 15, 15))]);
        let new = frame(&[(2, 1, Bounds::new(20, 1, 33, 15))]);
        let mut damage = DamageTiles::new(new.physical_size);
        diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
        assert_eq!(damage.list(), &[0, 1, 2]);
    }

    #[test]
    fn removal_delta_is_resolved_before_the_same_id_is_reinserted() {
        let mut removed = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
        let old = removed.nodes[0];
        removed.delta = Some(std::rc::Rc::new(RetainedFrameDelta {
            from_version: 1,
            to_version: 2,
            patches: vec![RetainedNodePatch {
                old: Some(old),
                new: None,
                damage: None,
            }]
            .into(),
            previous: None,
            depth: 1,
            damage: std::rc::Rc::new([]),
            dirty_backdrops: std::rc::Rc::new([]),
            backdrop_damage_complete: false,
            index: std::rc::Rc::new([(old.id, 0)].into_iter().collect()),
        }));
        let reinserted = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
        let mut damage = DamageTiles::new(reinserted.physical_size);

        diff_frames(
            &removed,
            &reinserted,
            &mut damage,
            &mut RetainedDamage::default(),
        );

        assert_eq!(damage.list(), &[1]);
    }

    #[test]
    fn overlay_only_insertion_participates_in_fallback_damage() {
        let previous = frame(&[]);
        let mut current = frame(&[]);
        let inserted = RetainedNodeState {
            id: RetainedNodeId::for_owner(2),
            revision: NodeGeneration::new(0),
            bounds: Bounds::new(32, 0, 48, 16),
            order: 0,
            kind: RetainedNodeKind::Scene,
            placement_bits: None,
        };
        current.delta = Some(std::rc::Rc::new(RetainedFrameDelta {
            from_version: 1,
            to_version: 2,
            patches: vec![RetainedNodePatch {
                old: None,
                new: Some(inserted),
                damage: None,
            }]
            .into(),
            previous: None,
            depth: 1,
            damage: std::rc::Rc::new([]),
            dirty_backdrops: std::rc::Rc::new([]),
            backdrop_damage_complete: false,
            index: std::rc::Rc::new([(inserted.id, 0)].into_iter().collect()),
        }));
        let mut damage = DamageTiles::new(current.physical_size);

        diff_frames(
            &previous,
            &current,
            &mut damage,
            &mut RetainedDamage::default(),
        );

        assert_eq!(damage.list(), &[2]);
    }

    #[test]
    fn compacted_state_equal_to_the_rebuilt_base_adds_no_damage() {
        let mut previous = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
        let logical = RetainedNodeState {
            revision: NodeGeneration::new(1),
            ..previous.nodes[0]
        };
        previous.state_pages = std::rc::Rc::new(
            [(0, std::rc::Rc::<[RetainedNodeState]>::from(vec![logical]))]
                .into_iter()
                .collect(),
        );
        let current = frame(&[(2, 1, Bounds::new(16, 0, 32, 16))]);
        let mut damage = DamageTiles::new(current.physical_size);

        diff_frames(
            &previous,
            &current,
            &mut damage,
            &mut RetainedDamage::default(),
        );

        assert!(damage.is_empty());
    }

    #[test]
    fn incomplete_previous_frame_cannot_become_an_incremental_baseline() {
        let mut previous = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        previous.incremental_complete = false;
        let current = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let mut state = IncrementalState {
            previous: Some(previous),
            renderer_state_invalid: false,
            frame_diff: Default::default(),
        };

        let plan = state.plan(
            Some(current),
            (128, 64),
            IncrementalRenderConfig::default(),
            true,
        );

        assert_eq!(
            plan.stats.full_redraw_reason,
            Some(FullRedrawReason::UntrackedContent)
        );
        assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
    }

    #[test]
    fn retained_layer_revision_change_dirties_its_bounds() {
        let mut previous = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
        Rc::make_mut(&mut previous.nodes)[0].kind = RetainedNodeKind::Layer;
        let mut current = previous.clone();
        Rc::make_mut(&mut current.nodes)[0].revision = NodeGeneration::new(1);
        let mut damage = DamageTiles::new(current.physical_size);

        diff_frames(
            &previous,
            &current,
            &mut damage,
            &mut RetainedDamage::default(),
        );

        assert_eq!(damage.list(), &[1]);
    }

    #[test]
    fn insertion_does_not_make_unchanged_siblings_look_reordered() {
        let old = frame(&[
            (2, 0, Bounds::new(0, 0, 16, 16)),
            (3, 0, Bounds::new(32, 0, 48, 16)),
        ]);
        let new = frame(&[
            (4, 0, Bounds::new(16, 0, 32, 16)),
            (2, 0, Bounds::new(0, 0, 16, 16)),
            (3, 0, Bounds::new(32, 0, 48, 16)),
        ]);
        let mut damage = DamageTiles::new(new.physical_size);
        diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
        assert_eq!(damage.list(), &[1]);
    }

    #[test]
    fn removal_dirties_the_removed_nodes_old_tiles() {
        let old = frame(&[
            (2, 0, Bounds::new(0, 0, 16, 16)),
            (3, 0, Bounds::new(32, 16, 48, 32)),
        ]);
        let new = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let mut damage = DamageTiles::new(new.physical_size);
        diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
        assert_eq!(damage.list(), &[10]);
    }

    #[test]
    fn reordering_dirties_both_overlapping_nodes() {
        let old = frame(&[
            (2, 0, Bounds::new(0, 0, 32, 16)),
            (3, 0, Bounds::new(16, 0, 48, 16)),
        ]);
        let new = frame(&[
            (3, 0, Bounds::new(16, 0, 48, 16)),
            (2, 0, Bounds::new(0, 0, 32, 16)),
        ]);
        let mut damage = DamageTiles::new(new.physical_size);
        diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
        assert_eq!(damage.list(), &[1]);
    }

    #[test]
    fn reordered_pair_spanning_many_tiles_is_recorded_once() {
        let old = frame(&[
            (2, 0, Bounds::new(0, 0, 64, 64)),
            (3, 0, Bounds::new(16, 16, 80, 64)),
        ]);
        let new = frame(&[
            (3, 0, Bounds::new(16, 16, 80, 64)),
            (2, 0, Bounds::new(0, 0, 64, 64)),
        ]);
        let mut damage = DamageTiles::new(new.physical_size);
        let mut retained = RetainedDamage::default();

        diff_frames(&old, &new, &mut damage, &mut retained);

        assert_eq!(retained.unattributed, [Bounds::new(16, 16, 64, 64)]);
        assert_eq!(damage.len(), 9);
    }

    #[test]
    fn reused_frame_diff_scratch_does_not_leak_old_tile_adjacency() {
        let overlapping = frame(&[
            (2, 0, Bounds::new(0, 0, 64, 64)),
            (3, 0, Bounds::new(16, 16, 80, 64)),
        ]);
        let overlapping_reversed = frame(&[
            (3, 0, Bounds::new(16, 16, 80, 64)),
            (2, 0, Bounds::new(0, 0, 64, 64)),
        ]);
        let mut scratch = FrameDiffScratch::default();
        diff_frames_reusing(
            &overlapping,
            &overlapping_reversed,
            &mut DamageTiles::new((128, 64)),
            &mut RetainedDamage::default(),
            &mut scratch,
        );

        let disjoint = frame(&[
            (2, 0, Bounds::new(0, 0, 16, 16)),
            (3, 0, Bounds::new(32, 0, 48, 16)),
        ]);
        let disjoint_reversed = frame(&[
            (3, 0, Bounds::new(32, 0, 48, 16)),
            (2, 0, Bounds::new(0, 0, 16, 16)),
        ]);
        let mut damage = DamageTiles::new((128, 64));
        let mut retained = RetainedDamage::default();
        diff_frames_reusing(
            &disjoint,
            &disjoint_reversed,
            &mut damage,
            &mut retained,
            &mut scratch,
        );

        assert!(damage.is_empty());
        assert!(retained.unattributed.is_empty());
    }

    #[test]
    fn reordering_non_overlapping_nodes_does_not_create_damage() {
        let old = frame(&[
            (2, 0, Bounds::new(0, 0, 16, 16)),
            (3, 0, Bounds::new(32, 0, 48, 16)),
        ]);
        let new = frame(&[
            (3, 0, Bounds::new(32, 0, 48, 16)),
            (2, 0, Bounds::new(0, 0, 16, 16)),
        ]);
        let mut damage = DamageTiles::new(new.physical_size);
        diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
        assert!(damage.is_empty());
    }

    #[test]
    fn spatial_reorder_matches_brute_force_for_every_ordering() {
        let nodes = [
            (2, 0, Bounds::new(-8, 0, 12, 24)),
            (3, 0, Bounds::new(4, 8, 28, 32)),
            (4, 0, Bounds::new(24, 0, 44, 20)),
            (5, 0, Bounds::new(36, 12, 60, 36)),
            (6, 0, Bounds::new(8, 28, 52, 52)),
        ];
        let previous = frame(&nodes);
        let old_rank = previous
            .nodes
            .iter()
            .enumerate()
            .map(|(rank, node)| (node.id, rank))
            .collect::<HashMap<_, _>>();
        let bounds = previous
            .nodes
            .iter()
            .map(|node| (node.id, node.bounds))
            .collect::<HashMap<_, _>>();
        let mut ids = nodes.map(|node| node.0);

        for_each_permutation(&mut ids, 0, &mut |order| {
            let current = frame(
                &order
                    .iter()
                    .map(|id| (*id, 0, bounds[&RetainedNodeId::for_owner(*id)]))
                    .collect::<Vec<_>>(),
            );
            let mut actual = DamageTiles::new(current.physical_size);
            diff_frames(
                &previous,
                &current,
                &mut actual,
                &mut RetainedDamage::default(),
            );

            let mut expected = DamageTiles::new(current.physical_size);
            for (new_rank_a, a) in current.nodes.iter().enumerate() {
                for b in &current.nodes[new_rank_a + 1..] {
                    if old_rank[&a.id] > old_rank[&b.id] {
                        expected.add_bounds(a.bounds.intersect(b.bounds));
                    }
                }
            }
            let mut actual = actual.list().to_vec();
            let mut expected = expected.list().to_vec();
            actual.sort_unstable();
            expected.sort_unstable();
            assert_eq!(actual, expected, "order {order:?}");
        });
    }

    fn for_each_permutation(values: &mut [u64], index: usize, visit: &mut dyn FnMut(&[u64])) {
        if index == values.len() {
            visit(values);
            return;
        }
        for next in index..values.len() {
            values.swap(index, next);
            for_each_permutation(values, index + 1, visit);
            values.swap(index, next);
        }
    }

    #[test]
    fn dirty_threshold_switches_to_the_full_fast_path() {
        let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let new = frame(&[(2, 1, Bounds::new(0, 0, 96, 64))]);
        let mut state = IncrementalState {
            previous: Some(old),
            renderer_state_invalid: false,
            frame_diff: Default::default(),
        };
        let plan = state.plan(
            Some(new),
            (128, 64),
            IncrementalRenderConfig {
                full_redraw_ratio: 0.7,
                ..Default::default()
            },
            true,
        );
        assert_eq!(
            plan.stats.full_redraw_reason,
            Some(FullRedrawReason::DirtyTileThreshold)
        );
        assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
    }

    #[test]
    fn stale_history_preserves_scene_damage_for_output_strategy() {
        let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let new = frame(&[(2, 1, Bounds::new(16, 0, 32, 16))]);
        let mut state = IncrementalState {
            previous: Some(old),
            renderer_state_invalid: false,
            frame_diff: Default::default(),
        };

        let plan = state.plan(
            Some(new),
            (128, 64),
            IncrementalRenderConfig::default(),
            false,
        );

        assert_eq!(
            plan.stats.full_redraw_reason,
            Some(FullRedrawReason::FirstFrame)
        );
        assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
        assert_eq!(plan.stats.changed_tiles, 2);
    }

    #[test]
    fn transient_output_uses_hysteresis_before_rebuilding_history() {
        let config = IncrementalRenderConfig::default();
        let stats = |changed_ratio, reason| IncrementalRenderStats {
            changed_ratio,
            full_redraw_reason: reason,
            ..Default::default()
        };
        let mut state = TransientOutputState::default();

        assert_eq!(
            state.decide(&stats(1.0, Some(FullRedrawReason::SurfaceChanged)), config),
            TransientOutputDecision::Direct
        );
        assert_eq!(
            state.decide(&stats(0.1, Some(FullRedrawReason::FirstFrame)), config),
            TransientOutputDecision::Direct
        );
        assert_eq!(
            state.decide(&stats(0.5, Some(FullRedrawReason::FirstFrame)), config),
            TransientOutputDecision::Direct
        );
        assert_eq!(
            state.decide(&stats(0.0, Some(FullRedrawReason::FirstFrame)), config),
            TransientOutputDecision::Direct
        );
        assert_eq!(
            state.decide(&stats(0.0, Some(FullRedrawReason::FirstFrame)), config),
            TransientOutputDecision::RebuildHistory
        );
    }

    #[test]
    fn first_frame_builds_history_instead_of_starting_direct_mode() {
        let mut state = TransientOutputState::default();
        let stats = IncrementalRenderStats {
            changed_ratio: 1.0,
            full_redraw_reason: Some(FullRedrawReason::FirstFrame),
            ..Default::default()
        };
        assert_eq!(
            state.decide(&stats, IncrementalRenderConfig::default()),
            TransientOutputDecision::InternalHistory
        );
    }

    #[test]
    fn surface_change_forces_full_redraw() {
        let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let mut resized = old.clone();
        resized.logical_size = (144, 64);
        resized.physical_size = (144, 64);
        let mut state = IncrementalState {
            previous: Some(old),
            renderer_state_invalid: false,
            frame_diff: Default::default(),
        };
        let plan = state.plan(
            Some(resized),
            (144, 64),
            IncrementalRenderConfig::default(),
            true,
        );
        assert_eq!(
            plan.stats.full_redraw_reason,
            Some(FullRedrawReason::SurfaceChanged)
        );
    }

    #[test]
    fn renderer_invalidation_reports_its_own_fallback_reason() {
        let retained = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let mut state = IncrementalState {
            previous: Some(retained.clone()),
            renderer_state_invalid: true,
            frame_diff: Default::default(),
        };
        let plan = state.plan(
            Some(retained),
            (128, 64),
            IncrementalRenderConfig::default(),
            false,
        );
        assert_eq!(
            plan.stats.full_redraw_reason,
            Some(FullRedrawReason::RendererStateChanged)
        );
    }

    #[test]
    fn active_scan_uses_persistent_chunk_allocations_after_a_path_is_removed() {
        use peniko::{
            Color,
            kurbo::{Affine, Rect, Shape},
        };

        use crate::{Canvas, FillRule, shared::gpu_plan::PersistentPathPlans};

        let mut canvas = Canvas::new(64, 16, 1.0);
        for x in [0.0, 16.0, 32.0] {
            canvas.push_path(
                Rect::new(x, 0.0, x + 12.0, 16.0).to_path(0.0),
                Color::BLACK,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.0,
            );
        }
        let mut plans = PersistentPathPlans::default();
        plans.update(&canvas, None);
        canvas.path_records[1] = Default::default();
        plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));

        let mut damage = DamageTiles::new((64, 16));
        damage.add_bounds(Bounds::new(32, 0, 48, 16));
        let active = ActiveScanPlan::new(&canvas, &damage, plans.scan_ranges());
        let chunks = &active.indices
            [active.chunk_base as usize..(active.chunk_base + active.chunk_count) as usize];

        assert_eq!(
            chunks,
            (plans.scan_ranges()[2].start..plans.scan_ranges()[2].end).collect::<Vec<_>>()
        );
    }
}
