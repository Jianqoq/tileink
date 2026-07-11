//! Retained-scene lifecycle and incremental history ownership.
//!
//! Keeping this state together prevents the GPU executor from independently mutating damage,
//! scene, surface, and presentation history that must advance as one transaction per frame.

use std::{collections::HashSet, sync::Arc};

use crate::{
    Canvas, SceneVersion,
    canvas::{RetainedFrame, RetainedSceneCache, RetainedSurfaceId},
    shared::bounds::Bounds,
};

use super::super::{
    incremental::{
        DamagePlan, DamageTiles, IncrementalRenderConfig, IncrementalRenderStats, IncrementalState,
        TransientOutputDecision, TransientOutputState,
    },
    retained_surfaces::{
        RetainedSurface, RetainedSurfaceCache, RetainedSurfaceKind, RetainedSurfaceMeta,
    },
};
use super::{ExternalTextureHistoryId, profile_cpu};

/// Coordinates CPU-side retained state while [`super::Renderer`] executes the resulting GPU work.
pub(super) struct RetainedRenderState {
    scene_cache: RetainedSceneCache,
    materialized: Option<CachedMaterializedScene>,
    prepared_scene: Option<u64>,
    next_materialization: u64,
    persistent_materialization: Option<(u64, u64)>,
    prepared_uses_text: bool,
    config: IncrementalRenderConfig,
    incremental: IncrementalState,
    stats: IncrementalRenderStats,
    active_tiles: Option<DamageTiles>,
    history_valid: bool,
    history_owner: HistoryOwner,
    transient_output: TransientOutputState,
    surfaces: RetainedSurfaceCache,
    rendering_frame: Option<RetainedFrame>,
    dirty_backdrop_nodes: HashSet<crate::RetainedNodeId>,
    persistent_frame: Option<(u64, SceneVersion, RetainedFrame)>,
    /// Last frame whose node membership was applied to the retained surface cache. Pointer
    /// identity makes unchanged frames O(1); a bridging journal delta updates removals directly.
    surface_frame: Option<RetainedFrame>,
}

impl RetainedRenderState {
    pub(super) fn new(config: IncrementalRenderConfig) -> Self {
        Self {
            scene_cache: RetainedSceneCache::default(),
            materialized: None,
            prepared_scene: None,
            next_materialization: 1,
            persistent_materialization: None,
            prepared_uses_text: false,
            config,
            incremental: IncrementalState::default(),
            stats: IncrementalRenderStats::default(),
            active_tiles: None,
            history_valid: false,
            history_owner: HistoryOwner::Internal,
            transient_output: TransientOutputState::default(),
            surfaces: RetainedSurfaceCache::new(config.retained_texture_budget_bytes),
            rendering_frame: None,
            dirty_backdrop_nodes: HashSet::new(),
            persistent_frame: None,
            surface_frame: None,
        }
    }

    pub(super) fn config(&self) -> IncrementalRenderConfig {
        self.config
    }

    pub(super) fn set_config(&mut self, config: IncrementalRenderConfig) {
        self.config = config.validate();
        self.surfaces
            .set_budget(self.config.retained_texture_budget_bytes);
    }

    pub(super) fn replace_mode(
        &mut self,
        mode: super::super::incremental::IncrementalRenderMode,
    ) -> super::super::incremental::IncrementalRenderMode {
        std::mem::replace(&mut self.config.mode, mode)
    }

    pub(super) fn stats(&self) -> &IncrementalRenderStats {
        &self.stats
    }

    pub(super) fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        &mut self.stats
    }

    pub(super) fn active_tiles(&self) -> Option<&DamageTiles> {
        self.active_tiles.as_ref()
    }

    /// High-damage transient/forced frames cannot benefit from backdrop history on the next
    /// equivalent frame. First-frame and surface-rebuild redraws are excluded because their
    /// surfaces seed the cache used by the following incremental frame.
    pub(super) fn bypasses_backdrop_cache(&self) -> bool {
        matches!(
            self.stats.full_redraw_reason,
            Some(
                super::super::incremental::FullRedrawReason::Forced
                    | super::super::incremental::FullRedrawReason::DirtyTileThreshold
            )
        )
    }

    pub(super) fn set_active_tiles(&mut self, active: Option<DamageTiles>) {
        self.active_tiles = active;
    }

    pub(super) fn take_active_tiles(&mut self) -> Option<DamageTiles> {
        self.active_tiles.take()
    }

    pub(super) fn invalidate(&mut self) {
        self.incremental.invalidate_renderer_state();
        self.history_valid = false;
        self.prepared_scene = None;
    }

    pub(super) fn invalidate_history(&mut self) {
        self.history_valid = false;
    }

    pub(super) fn invalidate_prepared_scene(&mut self) {
        self.prepared_scene = None;
    }

    pub(super) fn reset_transient_output(&mut self) {
        self.transient_output.reset();
    }

    pub(super) fn decide_transient_output(
        &mut self,
        stats: &IncrementalRenderStats,
    ) -> TransientOutputDecision {
        self.transient_output.decide(stats, self.config)
    }

    pub(super) fn set_history_owner(&mut self, owner: HistoryOwner) {
        if self.history_owner == owner {
            return;
        }
        self.history_owner = owner;
        self.history_valid = false;
        self.transient_output.reset();
    }

    pub(super) fn select_scene<'a>(&mut self, canvas: &'a Canvas) -> SelectedScene<'a> {
        let Some(frame) = profile_cpu("retained.collect", || canvas.retained_frame()) else {
            self.prepared_scene = None;
            return SelectedScene::Borrowed(canvas);
        };

        if frame.materialization_cacheable
            && let Some(cached) = &self.materialized
            && cached.frame.same_scene(&frame)
        {
            return SelectedScene::Retained {
                scene: cached.scene.clone(),
                frame,
                materialized_reused: true,
                materialization: cached.materialization,
            };
        }

        let scene = profile_cpu("retained.materialize", || {
            self.scene_cache.materialize_snapshot_shared(canvas)
        });
        self.scene_cache.retain_frame(&frame);
        let materialization = self.allocate_materialization();
        if frame.materialization_cacheable {
            self.materialized = Some(CachedMaterializedScene {
                frame: frame.clone(),
                scene: scene.clone(),
                materialization,
            });
        }
        SelectedScene::Retained {
            scene,
            frame,
            materialized_reused: false,
            materialization,
        }
    }

    pub(super) fn select_materialized(
        &mut self,
        scene: Arc<Canvas>,
        materialized_reused: bool,
        scene_id: u64,
        version: SceneVersion,
    ) -> SelectedScene<'static> {
        let materialization = if materialized_reused
            && let Some((cached_scene, materialization)) = self.persistent_materialization
            && cached_scene == scene_id
        {
            materialization
        } else {
            let materialization = self.allocate_materialization();
            self.persistent_materialization = Some((scene_id, materialization));
            materialization
        };
        let frame = if let Some((cached_id, cached_version, frame)) = &self.persistent_frame
            && (*cached_id, *cached_version) == (scene_id, version)
        {
            frame.clone()
        } else if materialized_reused
            && let Some((cached_id, _, cached)) = &self.persistent_frame
            && *cached_id == scene_id
        {
            // Raster-only invalidation installs an O(1) frame delta in the materialized Canvas.
            // Read that override so version cursors remain exact across later content commits.
            let _ = cached;
            let frame = scene
                .retained_frame()
                .expect("persistent materialized scene has retained identity");
            self.persistent_frame = Some((scene_id, version, frame.clone()));
            frame
        } else {
            let frame = profile_cpu("retained.collect", || {
                scene
                    .retained_frame()
                    .expect("persistent materialized scene has retained identity")
            });
            self.persistent_frame = Some((scene_id, version, frame.clone()));
            frame
        };
        SelectedScene::Retained {
            scene,
            frame,
            materialized_reused,
            materialization,
        }
    }

    fn allocate_materialization(&mut self) -> u64 {
        let id = self.next_materialization;
        self.next_materialization = self.next_materialization.wrapping_add(1).max(1);
        id
    }

    pub(super) fn begin_frame(
        &mut self,
        frame: Option<RetainedFrame>,
        scene: &Canvas,
        profiler_active: bool,
    ) -> DamagePlan {
        if self.surfaces.take_backdrop_evicted() {
            self.history_valid = false;
        }
        let physical_size = scene.physical_size();
        let mut plan = profile_cpu("retained.damage", || {
            self.incremental
                .plan(frame, physical_size, self.config, self.history_valid)
        });
        // Persistent deltas already contain ordinary layer influence and indexed backdrop
        // propagation. Snapshot canvases mark propagation as required and retain the generic
        // command-tree oracle; journal-connected scenes never need to rescan that tree.
        if plan.changed_tiles.len() < plan.changed_tiles.total_tiles()
            && plan
                .frame
                .as_ref()
                .is_none_or(|frame| frame.requires_damage_propagation)
        {
            if let Some(dirty) = &plan.dirty_backdrops {
                self.dirty_backdrop_nodes = dirty.iter().copied().collect();
            } else {
                let propagated = profile_cpu("retained.damage.propagate", || {
                    scene.propagate_damage(&plan.retained_damage)
                });
                self.dirty_backdrop_nodes = propagated.dirty_backdrops;
                plan.include_dependent_bounds(propagated.bounds, self.config);
            }
        } else {
            self.dirty_backdrop_nodes.clear();
        }
        if self.config.capture_active_tiles || profiler_active {
            plan.stats.active_tiles = plan.tiles.list().to_vec();
            plan.stats.active_tile_bounds = plan.tiles.coalesced_rects(physical_size);
        }
        self.stats = plan.stats.clone();
        self.active_tiles = (!plan.stats.full_redraw).then(|| plan.tiles.clone());
        self.rendering_frame = plan.frame.clone();
        plan
    }

    pub(super) fn finish_frame(&mut self, plan: DamagePlan, rendered: bool, history_updated: bool) {
        let backdrop_history_valid = !self.surfaces.take_backdrop_evicted();
        if rendered {
            if let Some(frame) = &plan.frame {
                let same_nodes = self
                    .surface_frame
                    .as_ref()
                    .is_some_and(|previous| Arc::ptr_eq(&previous.nodes, &frame.nodes));
                let bridging_delta = self.surface_frame.as_ref().and_then(|previous| {
                    let delta = frame.delta.as_ref()?;
                    (previous.root == frame.root
                        && previous.version == Some(delta.from_version)
                        && frame.version == Some(delta.to_version))
                    .then_some(delta)
                });
                if !same_nodes && let Some(delta) = bridging_delta {
                    let removed = delta
                        .patches
                        .iter()
                        .filter(|patch| patch.old.is_some() && patch.new.is_none())
                        .map(|patch| patch.old.unwrap().id)
                        .collect();
                    self.surfaces.remove_nodes(&removed);
                } else if !same_nodes {
                    self.stats.retained_surface_nodes_scanned = frame.nodes.len() as u32;
                    let nodes = frame.nodes.iter().map(|node| node.id).collect();
                    self.surfaces.retain_nodes(&nodes);
                }
                self.surface_frame = Some(frame.clone());
            }
            self.incremental.commit(plan.frame);
            self.history_valid = history_updated && backdrop_history_valid;
        } else {
            self.history_valid = false;
        }
        self.active_tiles = None;
        self.rendering_frame = None;
        self.dirty_backdrop_nodes.clear();
    }

    pub(super) fn scene_needs_prepare(
        &self,
        materialization: Option<u64>,
        uses_text: bool,
        resources_dirty: bool,
    ) -> bool {
        materialization.is_none()
            || self.prepared_scene != materialization
            || self.prepared_uses_text != uses_text
            || resources_dirty
    }

    pub(super) fn mark_scene_prepared(&mut self, materialization: Option<u64>, uses_text: bool) {
        self.prepared_scene = materialization;
        self.prepared_uses_text = uses_text;
    }

    pub(super) fn surface_meta(
        &self,
        id: RetainedSurfaceId,
        kind: RetainedSurfaceKind,
        size: (u32, u32),
        origin: (i32, i32),
        bounds: Bounds,
    ) -> Option<RetainedSurfaceMeta> {
        Some(RetainedSurfaceMeta {
            revision: self.rendering_frame.as_ref()?.node_revision(id.node)?,
            kind,
            size,
            origin,
            bounds,
        })
    }

    pub(super) fn surface_is_dirty(&self, bounds: Bounds) -> bool {
        self.active_tiles
            .as_ref()
            .is_none_or(|tiles| tiles.intersects_bounds(bounds))
    }

    pub(super) fn local_damage_for_surface(
        &self,
        surface: Bounds,
        root_size: (u32, u32),
    ) -> Option<DamageTiles> {
        let active = self.active_tiles.as_ref()?;
        let mut local = DamageTiles::new((surface.width(), surface.height()));
        for bounds in active.coalesced_rects(root_size) {
            let bounds = bounds.intersect(surface);
            if !bounds.is_empty() {
                local.add_bounds(Bounds::new(
                    bounds.x0 - surface.x0,
                    bounds.y0 - surface.y0,
                    bounds.x1 - surface.x0,
                    bounds.y1 - surface.y0,
                ));
            }
        }
        Some(local)
    }

    pub(super) fn take_matching_surface(
        &mut self,
        id: Option<RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
    ) -> Option<(RetainedSurfaceId, RetainedSurface)> {
        let id = id?;
        let surface = self.surfaces.take(id)?;
        (Some(surface.meta) == meta).then_some((id, surface))
    }

    pub(super) fn cache_surface(
        &mut self,
        id: Option<RetainedSurfaceId>,
        meta: Option<RetainedSurfaceMeta>,
        primary: super::super::target::WgpuTarget,
        secondary: Option<super::super::target::WgpuTarget>,
        backdrop_source: Option<super::super::target::WgpuTarget>,
    ) {
        if let (Some(id), Some(meta)) = (id, meta) {
            self.surfaces
                .insert(id, meta, primary, secondary, backdrop_source);
        }
    }

    pub(super) fn insert_surface(&mut self, id: RetainedSurfaceId, surface: RetainedSurface) {
        self.surfaces.insert(
            id,
            surface.meta,
            surface.primary,
            surface.secondary,
            surface.backdrop_source,
        );
    }

    pub(super) fn backdrop_is_dirty(&self, id: Option<RetainedSurfaceId>) -> bool {
        self.active_tiles.is_none()
            || id.is_none_or(|id| self.dirty_backdrop_nodes.contains(&id.node))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum HistoryOwner {
    #[default]
    Internal,
    External(ExternalTextureHistoryId),
}

struct CachedMaterializedScene {
    frame: RetainedFrame,
    scene: Arc<Canvas>,
    materialization: u64,
}

pub(super) enum SelectedScene<'a> {
    Borrowed(&'a Canvas),
    Retained {
        scene: Arc<Canvas>,
        frame: RetainedFrame,
        materialized_reused: bool,
        materialization: u64,
    },
}

impl SelectedScene<'_> {
    pub(super) fn scene(&self) -> &Canvas {
        match self {
            Self::Borrowed(scene) => scene,
            Self::Retained { scene, .. } => scene,
        }
    }

    pub(super) fn frame(&self) -> Option<RetainedFrame> {
        match self {
            Self::Borrowed(_) => None,
            Self::Retained { frame, .. } => Some(frame.clone()),
        }
    }

    pub(super) fn materialization(&self) -> Option<u64> {
        match self {
            Self::Borrowed(_) => None,
            Self::Retained {
                materialization, ..
            } => Some(*materialization),
        }
    }

    pub(super) fn materialized_reused(&self) -> bool {
        matches!(
            self,
            Self::Retained {
                materialized_reused: true,
                ..
            }
        )
    }
}
