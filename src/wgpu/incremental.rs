use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    Canvas, TILE_SIZE,
    canvas::{RetainedDamage, RetainedFrame, RetainedNodeState},
    shared::{
        bounds::Bounds,
        gpu_plan::{CUMSUM_CHUNK_SIZE, GpuCumsumPlan, SCAN_CHUNK_SIZE},
    },
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncrementalRenderMode {
    #[default]
    Auto,
    ForceFull,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncrementalRenderConfig {
    pub mode: IncrementalRenderMode,
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

#[derive(Clone, Debug)]
pub(crate) struct DamageTiles {
    tiles_width: u32,
    tiles_height: u32,
    bits: Vec<u64>,
    list: Vec<u32>,
}

impl DamageTiles {
    pub(crate) fn new(size: (u32, u32)) -> Self {
        let tiles_width = size.0.div_ceil(TILE_SIZE);
        let tiles_height = size.1.div_ceil(TILE_SIZE);
        let tile_count = tiles_width.saturating_mul(tiles_height);
        Self {
            tiles_width,
            tiles_height,
            bits: vec![0; tile_count.div_ceil(64) as usize],
            list: Vec::new(),
        }
    }

    pub(crate) fn full(size: (u32, u32)) -> Self {
        let mut damage = Self::new(size);
        let count = damage.total_tiles();
        damage.list.extend(0..count);
        for tile in 0..count {
            damage.bits[tile as usize / 64] |= 1 << (tile % 64);
        }
        damage
    }

    pub(crate) fn add_bounds(&mut self, bounds: Bounds) {
        let canvas = Bounds::canvas(
            self.tiles_width.saturating_mul(TILE_SIZE),
            self.tiles_height.saturating_mul(TILE_SIZE),
        );
        let bounds = bounds.intersect(canvas);
        if bounds.is_empty() {
            return;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        for y in y0..y1 {
            for x in x0..x1 {
                let tile = y * self.tiles_width + x;
                let word = &mut self.bits[tile as usize / 64];
                let mask = 1 << (tile % 64);
                if *word & mask == 0 {
                    *word |= mask;
                    self.list.push(tile);
                }
            }
        }
    }

    pub(crate) fn contains(&self, tile: u32) -> bool {
        self.bits
            .get(tile as usize / 64)
            .is_some_and(|word| *word & (1 << (tile % 64)) != 0)
    }

    pub(crate) fn intersects_bounds(&self, bounds: Bounds) -> bool {
        if bounds.is_empty() {
            return false;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        (y0..y1).any(|y| (x0..x1).any(|x| self.contains(y * self.tiles_width + x)))
    }

    pub(crate) fn intersects_tile_rect(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        let x1 = x1.min(self.tiles_width);
        let y1 = y1.min(self.tiles_height);
        (y0.min(y1)..y1).any(|y| (x0.min(x1)..x1).any(|x| self.contains(y * self.tiles_width + x)))
    }

    pub(crate) fn count_in_bounds(&self, bounds: Bounds) -> u32 {
        if bounds.is_empty() {
            return 0;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y * self.tiles_width + x))
            .filter(|tile| self.contains(*tile))
            .count() as u32
    }

    pub(crate) fn list(&self) -> &[u32] {
        &self.list
    }

    pub(crate) fn len(&self) -> u32 {
        self.list.len() as u32
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub(crate) fn total_tiles(&self) -> u32 {
        self.tiles_width.saturating_mul(self.tiles_height)
    }

    pub(crate) fn tiles_width(&self) -> u32 {
        self.tiles_width
    }

    /// Decomposes dirty tiles into non-overlapping pixel rectangles.
    ///
    /// Horizontal runs with the same extent on adjacent tile rows are merged
    /// vertically. GPU execution consumes `list` directly as one compact
    /// dispatch; rectangles remain useful for CPU damage propagation, bounds
    /// transforms, and diagnostics without expanding sparse damage to its
    /// bounding box.
    pub(crate) fn coalesced_rects(&self, physical_size: (u32, u32)) -> Vec<Bounds> {
        let mut rects = Vec::<Bounds>::new();
        let mut previous_row = HashMap::<(u32, u32), usize>::new();
        for y in 0..self.tiles_height {
            let mut current_row = HashMap::new();
            let mut x = 0;
            while x < self.tiles_width {
                while x < self.tiles_width && !self.contains(y * self.tiles_width + x) {
                    x += 1;
                }
                let start = x;
                while x < self.tiles_width && self.contains(y * self.tiles_width + x) {
                    x += 1;
                }
                if start < x {
                    let key = (start, x);
                    let y1 = ((y + 1) * TILE_SIZE).min(physical_size.1) as i32;
                    let index = if let Some(&index) = previous_row.get(&key) {
                        rects[index].y1 = y1;
                        index
                    } else {
                        let index = rects.len();
                        rects.push(Bounds::new(
                            (start * TILE_SIZE) as i32,
                            (y * TILE_SIZE) as i32,
                            (x * TILE_SIZE).min(physical_size.0) as i32,
                            y1,
                        ));
                        index
                    };
                    current_row.insert(key, index);
                }
            }
            previous_row = current_row;
        }
        rects
    }

    /// Expands pixel damage while preserving the tile-grid representation.
    ///
    /// Offscreen filters use this to redraw source pixels outside the visible
    /// output damage that can still be sampled into that output.
    pub(crate) fn outset(&self, physical_size: (u32, u32), pixels: i32) -> Self {
        if pixels <= 0 {
            return self.clone();
        }
        let canvas = Bounds::canvas(physical_size.0, physical_size.1);
        let mut expanded = Self::new(physical_size);
        for rect in self.coalesced_rects(physical_size) {
            expanded.add_bounds(rect.outset(pixels).intersect(canvas));
        }
        expanded
    }
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
    pub(crate) fn new(canvas: &Canvas, damage: &DamageTiles) -> Self {
        let mut lines = Vec::new();
        let mut paths = Vec::new();
        let mut chunks = Vec::new();
        let mut backdrops = Vec::new();
        let mut cumsum = GpuCumsumPlan::default();
        let mut chunk_cursor = 0u32;

        for record in &canvas.path_records {
            let chunk_count = record.data_len.div_ceil(SCAN_CHUNK_SIZE);
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
                chunks.extend(chunk_cursor..chunk_cursor.saturating_add(chunk_count));
                backdrops
                    .extend(record.data_offset..record.data_offset.saturating_add(record.data_len));
                append_cumsum_path(record, &mut cumsum);
            }
            chunk_cursor = chunk_cursor.saturating_add(chunk_count);
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
    pub(crate) dirty_backdrops: Option<std::sync::Arc<[crate::RetainedNodeId]>>,
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
            let size = (
                self.changed_tiles.tiles_width * TILE_SIZE,
                self.changed_tiles.tiles_height * TILE_SIZE,
            );
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
}

impl IncrementalState {
    pub(crate) fn invalidate_renderer_state(&mut self) {
        self.renderer_state_invalid = true;
    }

    pub(crate) fn clear_history(&mut self) {
        self.previous = None;
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
        &self,
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
                    let bounds = match (patch.old, patch.new) {
                        (Some(old), Some(new)) => old.bounds.union(new.bounds),
                        (Some(old), None) => old.bounds,
                        (None, Some(new)) => new.bounds,
                        (None, None) => continue,
                    };
                    tiles.add_bounds(bounds);
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
        if Arc::ptr_eq(&previous.nodes, &current.nodes) {
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
        diff_frames(previous, current, &mut tiles, &mut retained);
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

fn diff_frames(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
) {
    let old = previous
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let new = current
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();

    for node in current.nodes.iter() {
        let Some(previous) = old.get(&node.id) else {
            damage.add_bounds(node.bounds);
            retained.add_node(node.id, node.bounds);
            continue;
        };
        if previous.revision != node.revision
            || previous.kind != node.kind
            || previous.placement_bits != node.placement_bits
            || previous.bounds != node.bounds
        {
            let bounds = previous.bounds.union(node.bounds);
            damage.add_bounds(bounds);
            retained.add_node(node.id, bounds);
        }
    }
    for node in previous.nodes.iter() {
        if !new.contains_key(&node.id) {
            damage.add_bounds(node.bounds);
            // Removed commands have no insertion point in the current painter
            // order, so conservatively expose their old pixels to every
            // backdrop dependency.
            retained.add_unattributed(node.bounds);
        }
    }

    let common = old.keys().copied().collect::<HashSet<_>>();
    let old_order = common_order(&previous.nodes, &common, &new);
    let new_order = common_order(&current.nodes, &common, &old);
    if old_order != new_order {
        damage_reordered_nodes(&old_order, &new_order, &old, &new, damage, retained);
    }
}

/// Damages overlapping nodes whose painter order was inverted.
///
/// Reordering previously compared every common node pair, making even a reorder of disjoint
/// components quadratic. Tile buckets restrict comparisons to spatial neighbors. The remaining
/// worst case is intentionally output-sensitive: mutually overlapping reordered nodes can have
/// O(n²) real inversions, but processing stops as soon as every output tile is already dirty.
fn damage_reordered_nodes(
    old_order: &[crate::RetainedNodeId],
    new_order: &[crate::RetainedNodeId],
    old: &HashMap<crate::RetainedNodeId, &RetainedNodeState>,
    new: &HashMap<crate::RetainedNodeId, &RetainedNodeState>,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
) {
    let new_rank = ranks(new_order);
    let influence = old_order
        .iter()
        .map(|id| old[id].bounds.union(new[id].bounds))
        .collect::<Vec<_>>();
    let mut buckets = vec![Vec::new(); damage.total_tiles() as usize];
    let canvas = Bounds::canvas(
        damage.tiles_width.saturating_mul(TILE_SIZE),
        damage.tiles_height.saturating_mul(TILE_SIZE),
    );
    for (index, bounds) in influence.iter().enumerate() {
        let bounds = bounds.intersect(canvas);
        if bounds.is_empty() {
            continue;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(damage.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(damage.tiles_height);
        for y in y0..y1 {
            for x in x0..x1 {
                buckets[(y * damage.tiles_width + x) as usize].push(index);
            }
        }
    }

    let mut compared = HashSet::new();
    for bucket in buckets {
        if bucket
            .windows(2)
            .all(|pair| new_rank[&old_order[pair[0]]] < new_rank[&old_order[pair[1]]])
        {
            continue;
        }
        for (offset, &a) in bucket.iter().enumerate() {
            for &b in &bucket[offset + 1..] {
                if new_rank[&old_order[a]] < new_rank[&old_order[b]] || !compared.insert((a, b)) {
                    continue;
                }
                let bounds = influence[a].intersect(influence[b]);
                damage.add_bounds(bounds);
                retained.add_unattributed(bounds);
                if damage.len() == damage.total_tiles() {
                    return;
                }
            }
        }
    }
}

fn common_order(
    nodes: &[RetainedNodeState],
    candidates: &HashSet<crate::RetainedNodeId>,
    other: &HashMap<crate::RetainedNodeId, &RetainedNodeState>,
) -> Vec<crate::RetainedNodeId> {
    nodes
        .iter()
        .filter(|node| candidates.contains(&node.id) && other.contains_key(&node.id))
        .map(|node| node.id)
        .collect()
}

fn ranks(ids: &[crate::RetainedNodeId]) -> HashMap<crate::RetainedNodeId, usize> {
    ids.iter()
        .enumerate()
        .map(|(rank, id)| (*id, rank))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeGeneration, RetainedNodeId, canvas::RetainedNodeKind};

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
            node_index: std::sync::Arc::new(node_index),
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
    fn incomplete_previous_frame_cannot_become_an_incremental_baseline() {
        let mut previous = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        previous.incremental_complete = false;
        let current = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
        let mut state = IncrementalState {
            previous: Some(previous),
            renderer_state_invalid: false,
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
        Arc::make_mut(&mut previous.nodes)[0].kind = RetainedNodeKind::Layer;
        let mut current = previous.clone();
        Arc::make_mut(&mut current.nodes)[0].revision = NodeGeneration::new(1);
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
    fn coalesced_rects_merge_adjacent_dirty_tile_rows() {
        let mut damage = DamageTiles::new((50, 20));
        damage.add_bounds(Bounds::new(15, 0, 34, 17));
        assert_eq!(
            damage.coalesced_rects((50, 20)),
            vec![Bounds::new(0, 0, 48, 20)]
        );
        assert_eq!(damage.count_in_bounds(Bounds::new(16, 0, 32, 16)), 1);
    }

    #[test]
    fn coalesced_rects_keep_different_row_runs_separate() {
        let mut damage = DamageTiles::new((64, 48));
        damage.add_bounds(Bounds::new(0, 0, 32, 32));
        damage.add_bounds(Bounds::new(32, 16, 48, 48));

        assert_eq!(
            damage.coalesced_rects((64, 48)),
            vec![
                Bounds::new(0, 0, 32, 16),
                Bounds::new(0, 16, 48, 32),
                Bounds::new(32, 32, 48, 48),
            ]
        );
    }

    #[test]
    fn outset_includes_filter_source_tiles_beyond_visible_output() {
        let mut output = DamageTiles::new((328, 200));
        output.add_bounds(Bounds::new(112, 112, 256, 176));

        let source = output.outset((328, 200), 24);

        assert!(source.intersects_bounds(Bounds::new(112, 176, 256, 200)));
        assert_eq!(source.coalesced_rects((328, 200)).last().unwrap().y1, 200);
    }
}
