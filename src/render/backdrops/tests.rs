use super::*;
use crate::{
    Radius,
    render::{
        damage_tiles::DamageTiles,
        surfaces::{
            SurfaceAdapter,
            test_support::{Adapter, Allocation, Event, fixture, id},
        },
    },
    shared::layer::filter::{BlurSampling, COMPONENT_TRANSFER_TABLE_LEN},
};
use peniko::kurbo::{Affine, BezPath, Rect};

impl BackdropAdapter for Adapter {
    type WorkState = Option<DamageTiles>;
    fn try_direct_backdrop(
        &mut self,
        _: RenderTargetId,
        _: Bounds,
        filter: &Filter,
        _: &Region,
    ) -> Result<bool, Self::Error> {
        if matches!(filter, Filter::Blur { sampling, .. } if sampling.factor() > 1) {
            self.record(Event::DirectBackdrop, "direct")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn suspend_backdrop_work(&mut self) -> Self::WorkState {
        self.events.push(Event::SuspendBackdrop);
        self.retained.take_active_tiles()
    }
    fn restore_backdrop_work(&mut self, state: Self::WorkState) {
        self.events.push(Event::RestoreBackdrop);
        self.retained.set_active_tiles(state);
    }
    fn filter_backdrop(
        &mut self,
        pass: BackdropPass<'_>,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        self.record(
            Event::BackdropPass(
                pass.source,
                pass.target,
                pass.partial_output,
                self.retained.active_tiles().is_some(),
            ),
            "backdrop",
        )?;
        if let Filter::ComponentTransfer(_) = pass.filter {
            self.filter_transfer_indices
                .push(cursors.clone().next_transfer_index());
        }
        cursors.advance_filter(pass.filter);
        Ok(())
    }
    fn composite_backdrop_rect(
        &mut self,
        _: RenderTargetId,
        source: RenderTargetId,
        _: Bounds,
        region: &Region,
    ) -> Result<(), Self::Error> {
        assert!(matches!(region, Region::Rect { .. }));
        let RenderTargetId::Scratch(slot) = source else {
            panic!("scratch source")
        };
        self.record(
            Event::RectBackdrop(self.targets[slot].as_ref().unwrap().0),
            "composite",
        )
    }
    fn composite_cached_backdrop_rect(
        &mut self,
        _: RenderTargetId,
        source: &Allocation,
        _: Bounds,
        region: &Region,
    ) -> Result<(), Self::Error> {
        assert!(matches!(region, Region::Rect { .. }));
        self.record(Event::RectBackdrop(source.0), "composite")
    }
}

fn rect() -> Region {
    Region::Rect {
        rect: Rect::new(0.0, 0.0, 32.0, 32.0),
        radius: Radius::ZERO,
    }
}
fn path() -> Region {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((32.0, 0.0));
    path.line_to((0.0, 32.0));
    path.close_path();
    Region::Path {
        path,
        transform: Affine::IDENTITY,
        tolerance: 0.1,
    }
}
fn transfer() -> Filter {
    Filter::ComponentTransfer(Box::new([7; COMPONENT_TRANSFER_TABLE_LEN]))
}
#[allow(clippy::too_many_arguments)]
fn render(
    a: &mut Adapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    filter: &Filter,
    region: &Region,
    retained: bool,
    stack: Range<usize>,
) -> Result<FilterCursors, &'static str> {
    a.expected_stack = stack.clone();
    let mut cursors = FilterCursors::default();
    execute(
        a,
        canvas,
        plan,
        BackdropLayer {
            retained_id: retained.then(id),
            filter,
            region,
            stack,
            children: &plan.ops,
        },
        RenderTargetId::Main,
        &mut cursors,
    )?;
    Ok(cursors)
}
fn seed(a: &mut Adapter, mask: bool, history: bool) {
    let meta = a
        .retained
        .surface_meta(
            id(),
            RetainedSurfaceKind::Backdrop,
            a.size(),
            a.origin(),
            Bounds::canvas(32, 32),
        )
        .unwrap();
    a.retained.cache_surface(
        Some(id()),
        Some(meta),
        Allocation(20),
        mask.then_some(Allocation(30)),
        history.then_some(Allocation(40)),
    );
}
fn cached(a: &mut Adapter) -> Option<(u32, Option<u32>, Option<u32>)> {
    let meta = a.retained.surface_meta(
        id(),
        RetainedSurfaceKind::Backdrop,
        a.size(),
        a.origin(),
        Bounds::canvas(32, 32),
    );
    a.retained
        .take_matching_surface(Some(id()), meta)
        .map(|(_, s)| {
            (
                s.primary.0,
                s.secondary.map(|m| m.0),
                s.backdrop_source.map(|s| s.0),
            )
        })
}
fn idle(a: &Adapter) {
    assert!(a.targets.iter().all(Option::is_none));
}

#[test]
fn path_without_outer_clips_records_coverage_before_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    let mut cursors = render(&mut a, &canvas, &plan, &transfer(), &path(), false, 0..0).unwrap();
    let region = a
        .events
        .iter()
        .position(|e| matches!(e, Event::Region(Some(0))))
        .unwrap();
    let composite = a
        .events
        .iter()
        .position(|e| matches!(e, Event::Composite(None)))
        .unwrap();
    let foreground = a
        .events
        .iter()
        .position(|e| matches!(e, Event::Draw(RenderTargetId::Main)))
        .unwrap();
    assert!(region < composite && composite < foreground);
    assert!(!a.events.iter().any(|e| matches!(e, Event::RectBackdrop(_))));
    assert_eq!(a.filter_transfer_indices, vec![0, 1]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(cursors.next_path_index(&path()), Some(1));
    idle(&a);
}

#[test]
fn clean_path_cache_keeps_coverage_and_still_executes_dirty_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    seed(&mut a, true, true);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(20, 20, 24, 24));
    a.retained.set_active_tiles(Some(damage));
    let mut cursors = render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0).unwrap();
    assert_eq!(a.events[0], Event::Cached(20, Some(30), None));
    assert!(
        a.events
            .iter()
            .any(|e| matches!(e, Event::Draw(RenderTargetId::Main)))
    );
    assert!(
        !a.events
            .iter()
            .any(|e| matches!(e, Event::Acquire(_) | Event::BackdropPass(..)))
    );
    assert_eq!(a.filter_transfer_indices, vec![1]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(cursors.next_path_index(&path()), Some(1));
    assert_eq!(cached(&mut a), Some((20, Some(30), Some(40))));
    assert_eq!(a.retained.stats().reused_offscreen_surfaces, 1);
    idle(&a);
}

#[test]
fn rectangle_fast_path_is_used_only_without_outer_clips() {
    for stack in [0..0, 2..4] {
        let (canvas, plan) = fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
        render(
            &mut a,
            &canvas,
            &plan,
            &transfer(),
            &rect(),
            false,
            stack.clone(),
        )
        .unwrap();
        assert_eq!(
            a.events.iter().any(|e| matches!(e, Event::RectBackdrop(_))),
            stack.is_empty()
        );
        assert_eq!(
            a.events.iter().any(|e| matches!(e, Event::Region(None))),
            !stack.is_empty()
        );
        idle(&a);
    }
}

#[test]
fn empty_backdrop_consumes_its_resources_and_preserves_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    let region = Region::Path {
        path: BezPath::new(),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
    };
    let mut cursors = render(&mut a, &canvas, &plan, &transfer(), &region, true, 0..0).unwrap();
    assert_eq!(
        a.events,
        vec![
            Event::BeginRootBatch,
            Event::Draw(RenderTargetId::Main),
            Event::Filter
        ]
    );
    assert_eq!(a.filter_transfer_indices, vec![1]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(cursors.next_path_index(&path()), Some(1));
    idle(&a);
}

#[test]
fn failed_filter_or_coverage_restores_work_and_discards_incomplete_history() {
    for stage in ["copy", "backdrop", "region"] {
        let (canvas, plan) = fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
        // A cache miss in an incremental frame must build a complete history.
        let mut damage = DamageTiles::new((32, 32));
        damage.add_bounds(Bounds::new(2, 2, 4, 4));
        a.retained.set_active_tiles(Some(damage));
        a.fail = Some(stage);
        assert!(
            matches!(render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0), Err(error) if error == stage)
        );
        assert_eq!(a.retained.active_tiles().unwrap().len(), 1);
        assert!(cached(&mut a).is_none());
        assert!(!a.events.iter().any(|e| matches!(e, Event::Draw(_))));
        idle(&a);
        a.fail = None;
        render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0).unwrap();
        idle(&a);
    }
}

#[test]
fn exhausted_scratch_releases_every_already_acquired_slot() {
    for limit in 0..3 {
        let (canvas, plan) = fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
        a.scratch_limit = limit;
        assert!(matches!(
            render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0),
            Err("scratch")
        ));
        idle(&a);
        assert!(cached(&mut a).is_none());
        a.scratch_limit = 3;
        render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0).unwrap();
        idle(&a);
    }
}

#[test]
fn full_filter_rebuild_restores_active_work_before_composite() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(20, 20, 22, 22));
    a.retained.set_active_tiles(Some(damage));
    render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0).unwrap();
    assert!(a.events.contains(&Event::Copy(
        RenderTargetId::Main,
        RenderTargetId::Scratch(1),
        Bounds::canvas(32, 32)
    )));
    assert!(a.events.contains(&Event::BackdropPass(
        RenderTargetId::Scratch(1),
        RenderTargetId::Scratch(0),
        None,
        false
    )));
    let restored = a
        .events
        .iter()
        .position(|e| *e == Event::RestoreBackdrop)
        .unwrap();
    let composite = a
        .events
        .iter()
        .position(|e| matches!(e, Event::Composite(_)))
        .unwrap();
    assert!(restored < composite);
    assert_eq!(a.composites[0].3, Bounds::canvas(32, 32));
    assert_eq!(a.retained.active_tiles().unwrap().len(), 1);
    assert_eq!(cached(&mut a), Some((100, Some(102), Some(101))));
    idle(&a);
}

#[test]
fn composite_failure_keeps_complete_history_but_skips_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    a.fail = Some("composite");
    assert!(matches!(
        render(&mut a, &canvas, &plan, &transfer(), &path(), true, 0..0),
        Err("composite")
    ));
    assert!(!a.events.iter().any(|e| matches!(e, Event::Draw(_))));
    assert_eq!(cached(&mut a), Some((100, Some(102), Some(101))));
    idle(&a);
}

#[test]
fn cached_composite_failure_preserves_history_and_consumes_backdrop_resources() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    seed(&mut a, true, true);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(2, 2, 4, 4));
    a.retained.set_active_tiles(Some(damage));
    a.fail = Some("composite");
    a.expected_stack = 0..0;
    let mut cursors = FilterCursors::default();
    assert_eq!(
        execute(
            &mut a,
            &canvas,
            &plan,
            BackdropLayer {
                retained_id: Some(id()),
                filter: &transfer(),
                region: &path(),
                stack: 0..0,
                children: &plan.ops
            },
            RenderTargetId::Main,
            &mut cursors
        ),
        Err("composite")
    );
    assert_eq!(cursors.next_transfer_index(), 1);
    assert_eq!(cursors.next_path_index(&path()), Some(1));
    assert_eq!(cached(&mut a), Some((20, Some(30), Some(40))));
    assert!(!a.events.iter().any(|e| matches!(e, Event::Draw(_))));
    idle(&a);
}

#[test]
fn partial_blur_reuses_unfiltered_history_without_suspending_work() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    seed(&mut a, true, true);
    let filter = Filter::Blur {
        std_dev_x: 1.0,
        std_dev_y: 1.0,
        sampling: BlurSampling::default(),
    };
    render(&mut a, &canvas, &plan, &filter, &path(), true, 0..0).unwrap();
    assert!(
        a.events
            .contains(&Event::Install(RenderTargetId::Scratch(1), 40))
    );
    assert!(a.events.contains(&Event::BackdropPass(
        RenderTargetId::Scratch(1),
        RenderTargetId::Scratch(0),
        Some(Bounds::canvas(32, 32)),
        false
    )));
    assert!(!a.events.contains(&Event::SuspendBackdrop));
    assert_eq!(cached(&mut a), Some((20, Some(30), Some(40))));
    idle(&a);
}

#[test]
fn forced_redraw_does_not_capture_backdrop_history() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    a.retained.stats_mut().full_redraw_reason =
        Some(crate::render::incremental::FullRedrawReason::Forced);
    render(&mut a, &canvas, &plan, &transfer(), &rect(), true, 0..0).unwrap();
    assert!(!a.events.iter().any(|e| matches!(e, Event::Copy(..))));
    assert!(a.events.contains(&Event::BackdropPass(
        RenderTargetId::Main,
        RenderTargetId::Scratch(0),
        None,
        false
    )));
    assert!(cached(&mut a).is_none());
    idle(&a);
}

#[test]
fn nested_backdrop_foreground_preserves_its_scratch_target_and_root_budget() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    a.expected_stack = 0..0;
    let parent = a.acquire_scratch().unwrap();
    execute(
        &mut a,
        &canvas,
        &plan,
        BackdropLayer {
            retained_id: None,
            filter: &transfer(),
            region: &path(),
            stack: 0..0,
            children: &plan.ops,
        },
        parent,
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert_eq!(a.draw_batches, vec![(0, parent)]);
    assert!(!a.events.contains(&Event::BeginRootBatch));
    a.release_scratch(parent);
    idle(&a);
}

mod work;

#[test]
fn downsampled_rect_shortcut_preserves_foreground_and_obeys_cache_and_clip_rules() {
    for (retained, forced, outer_clip, path_region) in [
        (false, false, false, false),
        (true, true, false, false),
        (true, false, false, false),
        (false, false, true, false),
        (false, false, false, true),
    ] {
        let (canvas, plan) = fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
        if forced {
            a.retained.stats_mut().full_redraw_reason =
                Some(crate::render::incremental::FullRedrawReason::Forced);
        }
        let filter = Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: BlurSampling {
                factor: 2,
                ..Default::default()
            },
        };
        let region = if path_region { path() } else { rect() };
        let stack = if outer_clip { 2..4 } else { 0..0 };
        render(&mut a, &canvas, &plan, &filter, &region, retained, stack).unwrap();
        let shortcut = (!retained || forced) && !outer_clip && !path_region;
        assert_eq!(a.events.contains(&Event::DirectBackdrop), shortcut);
        if shortcut {
            assert_eq!(
                &a.events[..3],
                &[
                    Event::DirectBackdrop,
                    Event::BeginRootBatch,
                    Event::Draw(RenderTargetId::Main)
                ]
            );
            assert!(
                !a.events
                    .iter()
                    .any(|e| matches!(e, Event::Acquire(_) | Event::BackdropPass(..)))
            );
        }
        assert_eq!(a.filter_transfer_indices, vec![0]);
        idle(&a);
    }
}

#[test]
fn direct_backdrop_failure_aborts_without_fallback_or_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    a.fail = Some("direct");
    let filter = Filter::Blur {
        std_dev_x: 2.0,
        std_dev_y: 2.0,
        sampling: BlurSampling {
            factor: 2,
            ..Default::default()
        },
    };
    // A failed direct recording may already contain passes; retrying the ordinary
    // schedule would hide the failure and publish a partially modified frame.
    assert!(matches!(
        render(&mut a, &canvas, &plan, &filter, &rect(), false, 0..0),
        Err("direct")
    ));
    assert!(!a.events.iter().any(|e| matches!(
        e,
        Event::Acquire(_) | Event::BackdropPass(..) | Event::Draw(_)
    )));
    idle(&a);
}

#[test]
fn clean_rect_cache_uses_its_direct_composite_and_preserves_foreground() {
    let (canvas, plan) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    seed(&mut a, false, true);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(2, 2, 4, 4));
    a.retained.set_active_tiles(Some(damage));
    render(&mut a, &canvas, &plan, &transfer(), &rect(), true, 0..0).unwrap();
    assert_eq!(
        &a.events[..3],
        &[
            Event::RectBackdrop(20),
            Event::BeginRootBatch,
            Event::Draw(RenderTargetId::Main)
        ]
    );
    assert_eq!(cached(&mut a), Some((20, None, Some(40))));
    idle(&a);
}
