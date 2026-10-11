//! Current native redraw controls and diagnostics. Retired controls have no compatibility aliases.
use crate::shared::bounds::Bounds;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncrementalRenderMode {
    #[default]
    Auto,
    ForceFull,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncrementalRenderConfig {
    pub mode: IncrementalRenderMode,
    /// Dirty-tile ratio in `(0, 1]` that selects a full redraw.
    pub full_redraw_ratio: f32,
    /// Capture exact active tile IDs and regions in [`IncrementalRenderStats`].
    ///
    /// Disabled by default so ordinary rendering does not allocate diagnostic vectors per frame.
    pub capture_active_tiles: bool,
    /// Maximum retained offscreen texture storage, in bytes.
    pub retained_texture_budget_bytes: u64,
}

impl Default for IncrementalRenderConfig {
    fn default() -> Self {
        Self {
            mode: IncrementalRenderMode::Auto,
            full_redraw_ratio: 0.7,
            capture_active_tiles: false,
            retained_texture_budget_bytes: 256 * 1024 * 1024,
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
    ExplicitInvalidation,
    RendererStateChanged,
    DirtyTileThreshold,
    /// The rendered version predates a complete dependency damage history.
    DamageHistoryUnavailable,
}

/// Destination strategy used for the most recent retained frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncrementalOutputMode {
    /// Renderer-owned history was updated. A texture render may copy it to the caller afterward.
    #[default]
    InternalHistory,
    /// A caller-owned texture is itself the retained history.
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
    /// Exact active tile IDs when `capture_active_tiles` is enabled.
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
    /// Whether the renderer reused the already-flattened retained scene.
    pub materialized_scene_reused: bool,
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
    pub arena_live_bytes: u64,
    pub arena_capacity_bytes: u64,
    pub arena_fragmentation: f32,
    pub arena_compactions: u64,
}

#[cfg(test)]
mod tests;
