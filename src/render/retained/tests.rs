use super::*;
use crate::{
    NodeGeneration, RetainedNodeId,
    canvas::{RetainedNodeKind, RetainedNodeState},
    render::incremental::FullRedrawReason,
};

struct Allocation(u64);

impl SurfaceAllocation for Allocation {
    fn byte_len(&self) -> u64 {
        self.0
    }
}

fn state() -> RetainedRenderState<Allocation> {
    RetainedRenderState::new(IncrementalRenderConfig::default())
}

fn frame(revision: u64, bounds: Bounds) -> RetainedFrame {
    let node = RetainedNodeState {
        id: RetainedNodeId::for_owner(2),
        revision: NodeGeneration::new(revision),
        bounds,
        order: 0,
        kind: RetainedNodeKind::Scene,
        placement_bits: None,
    };
    RetainedFrame {
        root: RetainedNodeId::for_owner(1),
        logical_size: (128, 64),
        physical_size: (128, 64),
        scale_bits: 1.0f32.to_bits(),
        nodes: vec![node].into(),
        node_index: Rc::new([(node.id, 0)].into_iter().collect()),
        state_pages: Rc::new(Default::default()),
        invalidated_bounds: Vec::new(),
        invalidate_all: false,
        incremental_complete: true,
        version: None,
        delta: None,
        damage_history: crate::canvas::damage_history::DamageHistory::default(),
        dependency_free: true,
        requires_damage_propagation: false,
    }
}

fn commit(state: &mut RetainedRenderState<Allocation>, frame: &RetainedFrame) {
    let plan = state.begin_frame(Some(frame.clone()), &Canvas::new(128, 64, 1.0), false);
    state.finish_frame(plan, true, true);
}

#[test]
fn failed_frames_do_not_commit_damage_and_retry_rebuilds_history() {
    let mut state = state();
    let canvas = Canvas::new(128, 64, 1.0);
    let old = frame(1, Bounds::new(0, 0, 16, 16));
    let new = frame(2, Bounds::new(16, 0, 32, 16));
    commit(&mut state, &old);
    let failed = state.begin_frame(Some(new.clone()), &canvas, false);
    assert_eq!(failed.stats.changed_tiles, 2);
    state.finish_frame(failed, false, false);
    let retry = state.begin_frame(Some(new.clone()), &canvas, false);
    assert!(retry.stats.full_redraw);
    assert_eq!(retry.stats.changed_tiles, 2);
    state.finish_frame(retry, true, true);
    let unchanged = state.begin_frame(Some(new), &canvas, false);
    assert_eq!(unchanged.stats.dirty_tiles, 0);
    state.finish_frame(unchanged, true, true);
}

#[test]
fn persistent_history_requires_the_same_owner_and_successful_update() {
    let mut state = state();
    let canvas = Canvas::new(128, 64, 1.0);
    let frame = frame(1, Bounds::new(0, 0, 16, 16));
    let owner = HistoryOwner::External(ExternalTextureHistoryId::new(7));
    state.set_history_owner(owner);
    commit(&mut state, &frame);
    state.set_history_owner(owner);
    let unchanged = state.begin_frame(Some(frame.clone()), &canvas, false);
    assert_eq!(unchanged.stats.dirty_tiles, 0);
    state.finish_frame(unchanged, true, true);
    state.set_history_owner(HistoryOwner::External(ExternalTextureHistoryId::new(8)));
    let changed_owner = state.begin_frame(Some(frame.clone()), &canvas, false);
    assert!(changed_owner.stats.full_redraw);
    assert_eq!(changed_owner.stats.changed_tiles, 0);
    state.finish_frame(changed_owner, true, false);
    let missing_history = state.begin_frame(Some(frame), &canvas, false);
    assert!(missing_history.stats.full_redraw);
    state.finish_frame(missing_history, true, true);
}

#[test]
fn two_renderers_do_not_share_their_incremental_cursor() {
    let mut first = state();
    let mut second = state();
    let frame = frame(1, Bounds::new(0, 0, 16, 16));
    commit(&mut first, &frame);
    let first_plan = first.begin_frame(Some(frame.clone()), &Canvas::new(128, 64, 1.0), false);
    let second_plan = second.begin_frame(Some(frame), &Canvas::new(128, 64, 1.0), false);
    assert_eq!(first_plan.stats.dirty_tiles, 0);
    assert!(second_plan.stats.full_redraw);
    first.finish_frame(first_plan, true, true);
    second.finish_frame(second_plan, true, true);
}

#[test]
fn prepared_scene_reuse_checks_materialization_text_and_resource_changes() {
    let mut state = state();
    assert!(state.scene_needs_prepare(Some(1), false, false));
    state.mark_scene_prepared(Some(1), false);
    assert!(!state.scene_needs_prepare(Some(1), false, false));
    assert!(state.scene_needs_prepare(Some(2), false, false));
    assert!(state.scene_needs_prepare(Some(1), true, false));
    assert!(state.scene_needs_prepare(Some(1), false, true));
    state.mark_scene_prepared(None, false);
    assert!(state.scene_needs_prepare(None, false, false));
    state.mark_scene_prepared(Some(1), false);
    state.invalidate_prepared_scene();
    assert!(state.scene_needs_prepare(Some(1), false, false));
}

#[test]
fn local_damage_is_projected_from_root_tiles_and_keeps_empty_distinct_from_full() {
    let mut state = state();
    let surface = Bounds::new(-8, 8, 40, 56);
    assert!(state.local_damage_for_surface(surface, (64, 64)).is_none());
    state.set_active_tiles(Some(DamageTiles::new((64, 64))));
    assert!(
        state
            .local_damage_for_surface(surface, (64, 64))
            .unwrap()
            .is_empty()
    );
    let mut damage = DamageTiles::new((64, 64));
    damage.add_bounds(Bounds::new(16, 16, 32, 32));
    state.set_active_tiles(Some(damage));
    assert_eq!(
        state
            .local_damage_for_surface(surface, (64, 64))
            .unwrap()
            .list(),
        &[1, 2, 4, 5]
    );
}

fn surface_id() -> RetainedSurfaceId {
    RetainedSurfaceId {
        node: RetainedNodeId::for_owner(2),
        slot: 0,
    }
}

fn surface_meta() -> RetainedSurfaceMeta {
    RetainedSurfaceMeta {
        revision: NodeGeneration::new(1),
        kind: RetainedSurfaceKind::Filter,
        size: (16, 16),
        origin: (4, 8),
        bounds: Bounds::new(4, 8, 20, 24),
    }
}

#[test]
fn every_surface_metadata_component_participates_in_cache_identity() {
    let mut state = state();
    let id = surface_id();
    let meta = surface_meta();
    let alternatives = [
        RetainedSurfaceMeta {
            revision: NodeGeneration::new(2),
            ..meta
        },
        RetainedSurfaceMeta {
            kind: RetainedSurfaceKind::Mask,
            ..meta
        },
        RetainedSurfaceMeta {
            size: (17, 16),
            ..meta
        },
        RetainedSurfaceMeta {
            origin: (5, 8),
            ..meta
        },
        RetainedSurfaceMeta {
            bounds: Bounds::new(4, 8, 21, 24),
            ..meta
        },
    ];
    for different in alternatives {
        state.cache_surface(Some(id), Some(meta), Allocation(1024), None, None);
        assert!(
            state
                .take_matching_surface(Some(id), Some(different))
                .is_none()
        );
        assert!(state.take_matching_surface(Some(id), Some(meta)).is_none());
    }
    state.cache_surface(Some(id), Some(meta), Allocation(1024), None, None);
    assert!(state.take_matching_surface(Some(id), Some(meta)).is_some());
}

#[test]
fn evicting_a_backdrop_forces_the_next_root_to_rebuild() {
    let mut state = state();
    let frame = frame(1, Bounds::new(0, 0, 16, 16));
    commit(&mut state, &frame);
    let meta = RetainedSurfaceMeta {
        kind: RetainedSurfaceKind::Backdrop,
        ..surface_meta()
    };
    state.cache_surface(Some(surface_id()), Some(meta), Allocation(1024), None, None);
    let config = IncrementalRenderConfig {
        retained_texture_budget_bytes: 0,
        ..state.config()
    };
    state.set_config(config);
    let plan = state.begin_frame(Some(frame), &Canvas::new(128, 64, 1.0), false);
    assert!(plan.stats.full_redraw);
    state.finish_frame(plan, true, true);
}

#[test]
fn dirty_threshold_redraw_keeps_backdrop_history_for_the_next_incremental_frame() {
    let mut state = state();
    // Regression: sparse damage after a dirty-threshold frame still needs its
    // pre-backdrop pixels; final root pixels already contain the backdrop.
    state.stats.full_redraw_reason = Some(FullRedrawReason::DirtyTileThreshold);
    assert!(!state.bypasses_backdrop_cache());
    state.stats.full_redraw_reason = Some(FullRedrawReason::Forced);
    assert!(state.bypasses_backdrop_cache());
}

#[test]
fn growing_an_earlier_surface_preserves_unvisited_backdrop_input_in_the_current_frame() {
    let mut state = state();
    state.set_config(IncrementalRenderConfig {
        retained_texture_budget_bytes: 64,
        ..state.config()
    });
    let canvas = Canvas::new(128, 64, 1.0);
    let frame = frame(1, Bounds::new(0, 0, 16, 16));
    let first = state.begin_frame(Some(frame.clone()), &canvas, false);
    let filter_id = surface_id();
    let backdrop_id = RetainedSurfaceId {
        slot: 1,
        ..filter_id
    };
    let filter_meta = surface_meta();
    let backdrop_meta = RetainedSurfaceMeta {
        kind: RetainedSurfaceKind::Backdrop,
        bounds: Bounds::canvas(128, 64),
        size: (128, 64),
        ..filter_meta
    };
    state.cache_surface(
        Some(filter_id),
        Some(filter_meta),
        Allocation(8),
        None,
        None,
    );
    state.cache_surface(
        Some(backdrop_id),
        Some(backdrop_meta),
        Allocation(24),
        None,
        Some(Allocation(24)),
    );
    state.finish_frame(first, true, true);

    let mut changed = frame.clone();
    changed.invalidated_bounds.push(Bounds::new(1, 1, 2, 2));
    let second = state.begin_frame(Some(changed), &canvas, false);
    assert!(!second.stats.full_redraw);
    assert_eq!(second.tiles.len(), 1);
    assert!(
        state
            .take_matching_surface(Some(filter_id), Some(filter_meta))
            .is_some()
    );
    state.cache_surface(
        Some(filter_id),
        Some(filter_meta),
        Allocation(32),
        None,
        None,
    );
    // Losing this source now cannot be repaired from clean root pixels: those
    // already contain the previous backdrop and later foreground content.
    let (_, backdrop) = state
        .take_matching_surface(Some(backdrop_id), Some(backdrop_meta))
        .expect("the current partial frame still needs the later backdrop's painter-order input");
    assert_eq!(backdrop.backdrop_source.unwrap().0, 24);
    assert!(
        state
            .take_matching_surface(Some(filter_id), Some(filter_meta))
            .is_none(),
        "discard the new optional cache entry when only unvisited history can pay its budget"
    );
    state.finish_frame(second, true, true);
}

#[test]
fn failed_frame_completion_releases_backdrop_protection_before_budget_changes() {
    let mut state = state();
    let frame = frame(1, Bounds::new(0, 0, 16, 16));
    commit(&mut state, &frame);
    let meta = RetainedSurfaceMeta {
        kind: RetainedSurfaceKind::Backdrop,
        ..surface_meta()
    };
    state.cache_surface(
        Some(surface_id()),
        Some(meta),
        Allocation(32),
        None,
        Some(Allocation(32)),
    );
    let plan = state.begin_frame(Some(frame.clone()), &Canvas::new(128, 64, 1.0), false);
    assert!(!plan.stats.full_redraw);
    state.finish_frame(plan, false, false);
    state.set_config(IncrementalRenderConfig {
        retained_texture_budget_bytes: 0,
        ..state.config()
    });
    assert!(
        state
            .take_matching_surface(Some(surface_id()), Some(meta))
            .is_none()
    );
    let retry = state.begin_frame(Some(frame), &Canvas::new(128, 64, 1.0), false);
    assert!(retry.stats.full_redraw);
    state.finish_frame(retry, true, true);
}
