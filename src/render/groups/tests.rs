use super::*;
use crate::render::{damage_tiles::DamageTiles, surfaces::test_support::*};
impl GroupAdapter for Adapter {
    fn apply_opacity(
        &mut self,
        _: RenderTargetId,
        _: Bounds,
        opacity: f32,
    ) -> Result<(), Self::Error> {
        self.record(Event::Opacity(opacity), "opacity")
    }
    fn build_mask(&mut self, _: RenderTargetId, draw: u32, _: Bounds) -> Result<(), Self::Error> {
        self.record(Event::Mask(draw), "mask")
    }
}

fn group(plan: &ExecPlan, retained: bool) -> Group<'_> {
    Group {
        retained_id: retained.then(id),
        draw: 0,
        outer_stack: 2..4,
        children: &plan.ops,
        opacity: None,
        blend: None,
    }
}

#[test]
fn uncached_group_clears_renders_then_masks_and_composites_in_order() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    let mut op = group(&plan, false);
    op.opacity = Some(0.5);
    let mut cursors = FilterCursors::default();
    execute(
        &mut adapter,
        &canvas,
        &plan,
        op,
        RenderTargetId::Main,
        &mut cursors,
    )
    .unwrap();
    let (source, mask) = (RenderTargetId::Scratch(0), RenderTargetId::Scratch(1));
    assert_eq!(
        adapter.events,
        vec![
            Event::Acquire(source),
            Event::Clear(source),
            Event::Draw(source),
            Event::Filter,
            Event::Opacity(0.5),
            Event::Acquire(mask),
            Event::Mask(0),
            Event::Composite(None),
            Event::Release(mask),
            Event::Release(source)
        ]
    );
    assert_eq!(cursors.next_transfer_index(), 1);
    assert!(adapter.targets.iter().all(Option::is_none));
}

#[test]
fn clean_cached_group_keeps_ownership_and_skips_all_child_work() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.cached();
    adapter
        .retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    let mut op = group(&plan, true);
    op.blend = Some(peniko::BlendMode::default());
    let mut cursors = FilterCursors::default();
    execute(
        &mut adapter,
        &canvas,
        &plan,
        op,
        RenderTargetId::Main,
        &mut cursors,
    )
    .unwrap();
    assert_eq!(
        adapter.events,
        vec![Event::Cached(
            20,
            Some(30),
            Some(peniko::BlendMode::default())
        )]
    );
    assert_eq!(cursors.next_transfer_index(), 1);
    assert_eq!(adapter.retained.stats().reused_offscreen_surfaces, 1);
    assert_eq!(adapter.cached_ids(1), (20, 30));
}

#[test]
fn dirty_cached_group_reuses_both_surfaces_and_counts_only_active_tiles() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.cached();
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(0, 0, 8, 8));
    adapter.retained.set_active_tiles(Some(damage));
    execute(
        &mut adapter,
        &canvas,
        &plan,
        group(&plan, true),
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert!(
        adapter
            .events
            .contains(&Event::Install(RenderTargetId::Scratch(0), 20))
    );
    assert!(
        adapter
            .events
            .contains(&Event::Install(RenderTargetId::Scratch(1), 30))
    );
    assert!(adapter.events.contains(&Event::ClearRegion(
        RenderTargetId::Scratch(0),
        Bounds::canvas(32, 32)
    )));
    assert!(
        !adapter
            .events
            .iter()
            .any(|event| matches!(event, Event::Clear(_)))
    );
    assert_eq!(adapter.retained.stats().rerendered_offscreen_tiles, 1);
    assert_eq!(adapter.cached_ids(1), (20, 30));
}

#[test]
fn a_new_generation_cannot_reuse_the_previous_group_image() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.cached();
    adapter.retained.begin_frame(Some(frame(2)), &canvas, false);
    execute(
        &mut adapter,
        &canvas,
        &plan,
        group(&plan, true),
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert!(
        !adapter
            .events
            .iter()
            .any(|event| matches!(event, Event::Cached(..) | Event::Install(..)))
    );
    assert_eq!(adapter.cached_ids(2), (100, 101));
    assert_eq!(adapter.retained.stats().rerendered_offscreen_tiles, 4);
}

#[test]
fn no_scratch_capacity_rejects_before_any_child_or_composite_work() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.scratch_limit = 0;
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, false),
            RenderTargetId::Main,
            &mut FilterCursors::default()
        ),
        Err("scratch")
    );
    assert!(adapter.events.is_empty());
}

#[test]
fn failed_children_release_the_new_source_and_do_not_reach_mask_or_composite() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.fail = Some("children");
    let mut cursors = FilterCursors::default();
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, false),
            RenderTargetId::Main,
            &mut cursors
        ),
        Err("children")
    );
    assert_eq!(cursors.next_transfer_index(), 0);
    assert_eq!(
        adapter.events.last(),
        Some(&Event::Release(RenderTargetId::Scratch(0)))
    );
    assert!(
        !adapter
            .events
            .iter()
            .any(|event| matches!(event, Event::Filter | Event::Mask(_) | Event::Composite(_)))
    );
    assert!(adapter.targets.iter().all(Option::is_none));
}

#[test]
fn mask_allocation_failure_releases_the_rendered_source() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.scratch_limit = 1;
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, false),
            RenderTargetId::Main,
            &mut FilterCursors::default()
        ),
        Err("scratch")
    );
    assert_eq!(
        adapter.events.last(),
        Some(&Event::Release(RenderTargetId::Scratch(0)))
    );
    assert!(
        !adapter
            .events
            .iter()
            .any(|event| matches!(event, Event::Mask(_) | Event::Composite(_)))
    );
    assert!(adapter.targets.iter().all(Option::is_none));
}

#[test]
fn composite_failure_is_returned_after_uncached_targets_are_released() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.fail = Some("composite");
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, false),
            RenderTargetId::Main,
            &mut FilterCursors::default()
        ),
        Err("composite")
    );
    assert!(adapter.targets.iter().all(Option::is_none));
}

#[test]
fn cached_composite_failure_still_retains_its_source_and_advances_cursors() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.cached();
    adapter
        .retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    adapter.fail = Some("composite");
    let mut cursors = FilterCursors::default();
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, true),
            RenderTargetId::Main,
            &mut cursors
        ),
        Err("composite")
    );
    assert_eq!(cursors.next_transfer_index(), 1);
    assert_eq!(adapter.cached_ids(1), (20, 30));
}

#[test]
fn tile_accounting_clips_negative_bounds_covers_edges_and_saturates() {
    for (bounds, count) in [
        (Bounds::new(0, 0, 0, 16), 0),
        (Bounds::new(-16, -16, 0, 0), 0),
        (Bounds::new(-1, -1, 1, 1), 1),
        (Bounds::new(1, 1, 17, 17), 4),
        (Bounds::new(0, 0, i32::MAX, i32::MAX), u32::MAX),
    ] {
        assert_eq!(tile_count_for_bounds(bounds), count);
    }
}

// The shared adapter introduces fallible recording operations. Every local
// allocation must become reusable when execution returns, including error exits.
#[test]
fn fallible_recording_releases_uncached_scratch_and_allows_a_same_pool_retry() {
    for stage in ["clear", "opacity", "mask"] {
        let (canvas, plan) = fixture();
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
        adapter.scratch_limit = 2;
        adapter.fail = Some(stage);
        let mut op = group(&plan, false);
        op.opacity = Some(0.5);
        assert_eq!(
            execute(
                &mut adapter,
                &canvas,
                &plan,
                op,
                RenderTargetId::Main,
                &mut FilterCursors::default()
            ),
            Err(stage)
        );
        assert!(
            adapter.targets.iter().all(Option::is_none),
            "{stage} must return all acquired scratch"
        );
        adapter.fail = None;
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, false),
            RenderTargetId::Main,
            &mut FilterCursors::default(),
        )
        .unwrap();
        assert!(adapter.targets.iter().all(Option::is_none));
    }
}

#[test]
fn failed_cached_recording_returns_both_surfaces_and_allows_a_same_pool_retry() {
    for stage in ["clear", "children", "opacity", "mask"] {
        let (canvas, plan) = fixture();
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
        adapter.cached();
        adapter.scratch_limit = 2;
        adapter.fail = Some(stage);
        let mut op = group(&plan, true);
        op.opacity = Some(0.5);
        assert_eq!(
            execute(
                &mut adapter,
                &canvas,
                &plan,
                op,
                RenderTargetId::Main,
                &mut FilterCursors::default()
            ),
            Err(stage)
        );
        assert!(
            adapter.targets.iter().all(Option::is_none),
            "cached {stage} must return all acquired scratch"
        );
        adapter.fail = None;
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, true),
            RenderTargetId::Main,
            &mut FilterCursors::default(),
        )
        .unwrap();
        assert_eq!(adapter.cached_ids(1), (100, 101));
        assert!(adapter.targets.iter().all(Option::is_none));
    }
}

#[test]
fn cached_mask_allocation_failure_releases_its_source_and_can_retry() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.cached();
    adapter.scratch_limit = 1;
    assert_eq!(
        execute(
            &mut adapter,
            &canvas,
            &plan,
            group(&plan, true),
            RenderTargetId::Main,
            &mut FilterCursors::default()
        ),
        Err("scratch")
    );
    assert!(adapter.targets.iter().all(Option::is_none));
    adapter.scratch_limit = 2;
    execute(
        &mut adapter,
        &canvas,
        &plan,
        group(&plan, true),
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert_eq!(adapter.cached_ids(1), (100, 101));
}

// Compositing failure does not leave local scratch ownership behind. Recorded
// surfaces retain the reference cache handoff; failed-frame readiness is separate.
#[test]
fn failed_composite_transfers_recorded_surfaces_to_the_cache_once() {
    for was_cached in [false, true] {
        let (canvas, plan) = fixture();
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
        adapter.scratch_limit = 2;
        if was_cached {
            adapter.cached();
        }
        adapter.fail = Some("composite");
        let mut cursors = FilterCursors::default();
        assert_eq!(
            execute(
                &mut adapter,
                &canvas,
                &plan,
                group(&plan, true),
                RenderTargetId::Main,
                &mut cursors
            ),
            Err("composite")
        );
        assert_eq!(cursors.next_transfer_index(), 1);
        assert!(adapter.targets.iter().all(Option::is_none));
        assert!(
            !adapter
                .events
                .iter()
                .any(|event| matches!(event, Event::Release(_)))
        );
        assert_eq!(
            adapter.cached_ids(1),
            if was_cached { (20, 30) } else { (100, 101) }
        );
    }
}
