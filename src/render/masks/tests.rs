use super::*;
use crate::Radius;
use crate::render::retained_surfaces::RetainedSurfaceKind;
use crate::render::{damage_tiles::DamageTiles, surfaces::test_support::*};
use crate::shared::layer::mask::Mask;
use peniko::kurbo::Rect;
impl MaskAdapter for Adapter {
    fn mask_coverage(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        kind: MaskKind,
    ) -> Result<(), Self::Error> {
        self.mask_calls
            .push(MaskCall::Coverage(source, target, bounds, kind));
        self.record(Event::Coverage(kind), "coverage")
    }
    fn region_mask(
        &mut self,
        target: RenderTargetId,
        _: &Region,
        path: Option<u32>,
        bounds: Bounds,
    ) -> Result<(), Self::Error> {
        self.mask_calls.push(MaskCall::Region(target, path, bounds));
        self.record(Event::Region(path), "region")
    }
    fn apply_region(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), Self::Error> {
        self.mask_calls
            .push(MaskCall::Apply(source, target, bounds));
        self.record(Event::Apply, "apply")
    }
}
fn layer(kind: MaskKind) -> Mask {
    Mask {
        region: Region::Rect {
            rect: Rect::new(0.0, 0.0, 32.0, 32.0),
            radius: Radius::ZERO,
        },
        kind,
    }
}
fn op<'a>(plan: &'a ExecPlan, layer: &'a Mask, retained: bool) -> Masked<'a> {
    Masked {
        retained_id: retained.then(id),
        layer,
        outer_stack: 2..4,
        content: &plan.ops[..2],
        mask: &plan.ops[2..],
    }
}
fn render(
    adapter: &mut Adapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    layer: &Mask,
    retained: bool,
) -> Result<(), &'static str> {
    execute(
        adapter,
        canvas,
        plan,
        op(plan, layer, retained),
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
}
#[test]
fn uncached_masks_preserve_content_coverage_region_and_composite_order() {
    for kind in [MaskKind::Alpha, MaskKind::Luminance] {
        let (canvas, plan) = mask_fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
        let s = RenderTargetId::Scratch;
        render(&mut a, &canvas, &plan, &layer(kind), false).unwrap();
        assert_eq!(
            a.events,
            vec![
                Event::Acquire(s(0)),
                Event::Clear(s(0)),
                Event::Draw(s(0)),
                Event::Filter,
                Event::Acquire(s(1)),
                Event::Clear(s(1)),
                Event::Draw(s(1)),
                Event::Filter,
                Event::Acquire(s(2)),
                Event::Coverage(kind),
                Event::Release(s(1)),
                Event::Acquire(s(1)),
                Event::Region(None),
                Event::Apply,
                Event::Release(s(1)),
                Event::Composite(None),
                Event::Release(s(2)),
                Event::Release(s(0))
            ]
        );
        assert_eq!(a.draw_batches, vec![(0, s(0)), (1, s(1))]);
        assert_eq!(a.filters, vec![11, 22]);
        let bounds = Bounds::canvas(32, 32);
        assert_eq!(
            a.mask_calls,
            vec![
                MaskCall::Coverage(s(1), s(2), bounds, kind),
                MaskCall::Region(s(1), None, bounds),
                MaskCall::Apply(s(1), s(2), bounds)
            ]
        );
        assert_eq!(
            a.composites,
            vec![(RenderTargetId::Main, s(0), s(2), bounds)]
        );
        assert!(a.targets.iter().all(Option::is_none));
    }
}
#[test]
fn clean_cached_mask_skips_work_but_advances_both_resource_sequences() {
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    a.cached();
    a.retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    let mut cursors = FilterCursors::default();
    execute(
        &mut a,
        &canvas,
        &plan,
        op(&plan, &layer(MaskKind::Alpha), true),
        RenderTargetId::Main,
        &mut cursors,
    )
    .unwrap();
    assert_eq!(a.events, vec![Event::Cached(20, Some(30), None)]);
    assert_eq!(a.cached_ids(1), (20, 30));
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(a.retained.stats().reused_offscreen_surfaces, 1);
}
#[test]
fn dirty_cached_mask_reuses_content_and_coverage_with_active_tile_accounting() {
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    a.cached();
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(0, 0, 8, 8));
    a.retained.set_active_tiles(Some(damage));
    render(&mut a, &canvas, &plan, &layer(MaskKind::Luminance), true).unwrap();
    assert_eq!(a.cached_ids(1), (20, 30));
    assert_eq!(a.retained.stats().rerendered_offscreen_tiles, 1);
    assert!(a.events.contains(&Event::ClearRegion(
        RenderTargetId::Scratch(0),
        Bounds::canvas(32, 32)
    )));
    assert_eq!(
        a.events
            .iter()
            .filter(|e| matches!(e, Event::Clear(_)))
            .count(),
        1
    );
    assert!(a.targets.iter().all(Option::is_none));
}
#[test]
fn cached_mask_source_failure_returns_all_slots_and_allows_retry() {
    // Root cause: the cached coverage slot used to remain occupied when mask-source
    // allocation failed after content rendering. A retry must use the same pool.
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    a.cached();
    a.scratch_limit = 2;
    assert_eq!(
        render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true),
        Err("scratch")
    );
    assert!(a.targets.iter().all(Option::is_none));
    a.scratch_limit = 3;
    render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true).unwrap();
    assert_eq!(a.cached_ids(1), (100, 102));
    assert!(a.targets.iter().all(Option::is_none));
}
#[test]
fn every_recording_failure_releases_scratch_and_allows_a_same_pool_retry() {
    for retained in [false, true] {
        for stage in ["clear", "children", "filter", "coverage", "region", "apply"] {
            let (canvas, plan) = mask_fixture();
            let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
            if retained {
                a.cached();
            }
            a.fail = Some(stage);
            assert_eq!(
                render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), retained),
                Err(stage)
            );
            assert!(
                a.targets.iter().all(Option::is_none),
                "retained={retained} stage={stage}"
            );
            assert!(!a.events.iter().any(|e| matches!(e, Event::Composite(_))));
            a.fail = None;
            render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), retained).unwrap();
            assert!(a.targets.iter().all(Option::is_none));
        }
    }
}
#[test]
fn composite_failure_keeps_valid_cached_images_without_occupied_slots() {
    for cached in [false, true] {
        let (canvas, plan) = mask_fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
        if cached {
            a.cached();
        }
        a.fail = Some("composite");
        assert_eq!(
            render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true),
            Err("composite")
        );
        assert_eq!(a.cached_ids(1), if cached { (20, 30) } else { (100, 102) });
        assert!(a.targets.iter().all(Option::is_none));
    }
}
#[test]
fn empty_mask_advances_resources_without_allocation_or_child_work() {
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    let mut layer = layer(MaskKind::Alpha);
    layer.region = Region::Rect {
        rect: Rect::new(50.0, 50.0, 60.0, 60.0),
        radius: Radius::ZERO,
    };
    let mut cursors = FilterCursors::default();
    execute(
        &mut a,
        &canvas,
        &plan,
        op(&plan, &layer, true),
        RenderTargetId::Main,
        &mut cursors,
    )
    .unwrap();
    assert!(a.events.is_empty());
    assert_eq!(cursors.next_transfer_index(), 2);
}

#[test]
fn failure_while_rendering_mask_source_releases_cached_coverage_too() {
    for stage in ["clear", "children", "filter"] {
        let (canvas, plan) = mask_fixture();
        let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
        a.cached();
        a.fail = Some(stage);
        a.skip_failures = 1;
        assert_eq!(
            render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true),
            Err(stage)
        );
        assert!(
            a.events
                .contains(&Event::Install(RenderTargetId::Scratch(1), 30))
        );
        assert!(a.targets.iter().all(Option::is_none), "mask source {stage}");
        a.fail = None;
        render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true).unwrap();
        assert!(a.targets.iter().all(Option::is_none));
    }
}
#[test]
fn mask_generation_change_replaces_cached_images() {
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    a.cached();
    a.retained.begin_frame(Some(frame(2)), &canvas, false);
    render(&mut a, &canvas, &plan, &layer(MaskKind::Alpha), true).unwrap();
    assert!(
        !a.events
            .iter()
            .any(|e| matches!(e, Event::Cached(..) | Event::Install(..)))
    );
    assert_eq!(a.cached_ids(2), (100, 102));
    assert_eq!(a.retained.stats().rerendered_offscreen_tiles, 4);
}
#[test]
fn path_region_advances_once_when_empty_cached_or_rendered() {
    use peniko::kurbo::{Affine, Shape};
    for empty in [false, true] {
        for clean in [false, true] {
            let (canvas, plan) = mask_fixture();
            let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
            let mut layer = layer(MaskKind::Alpha);
            layer.region = Region::Path {
                path: Rect::new(0.0, 0.0, 32.0, 32.0).to_path(0.1),
                transform: if empty {
                    Affine::translate((64.0, 64.0))
                } else {
                    Affine::IDENTITY
                },
                tolerance: 0.1,
            };
            if clean {
                a.cached();
                a.retained
                    .set_active_tiles(Some(DamageTiles::new((32, 32))));
            }
            let mut cursors = FilterCursors::default();
            execute(
                &mut a,
                &canvas,
                &plan,
                op(&plan, &layer, true),
                RenderTargetId::Main,
                &mut cursors,
            )
            .unwrap();
            assert_eq!(cursors.next_path_index(&layer.region), Some(1));
            assert_eq!(cursors.next_transfer_index(), 2);
            if !clean && !empty {
                assert!(a.events.contains(&Event::Region(Some(0))));
            }
        }
    }
}
#[test]
fn clean_cached_composite_failure_preserves_images_and_resource_cursors() {
    let (canvas, plan) = mask_fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Mask);
    a.cached();
    a.retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    a.fail = Some("composite");
    let mut cursors = FilterCursors::default();
    assert_eq!(
        execute(
            &mut a,
            &canvas,
            &plan,
            op(&plan, &layer(MaskKind::Alpha), true),
            RenderTargetId::Main,
            &mut cursors
        ),
        Err("composite")
    );
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(a.cached_ids(1), (20, 30));
    assert!(a.targets.iter().all(Option::is_none));
}

fn mask_fixture() -> (Canvas, ExecPlan) {
    use crate::shared::{
        execution::ExecOp,
        layer::{
            Layer,
            filter::{COMPONENT_TRANSFER_TABLE_LEN, Filter},
        },
    };
    use std::rc::Rc;
    let (mut canvas, mut plan) = fixture();
    canvas.push_rect(
        Rect::new(4.0, 4.0, 12.0, 12.0),
        Radius::ZERO,
        peniko::Color::BLACK,
    );
    let mut mask = plan.ops.clone();
    let ExecOp::DrawBatch {
        draws, batch_id, ..
    } = &mut mask[0]
    else {
        unreachable!()
    };
    *draws = Rc::new(vec![1]);
    *batch_id = 1;
    let ExecOp::OffscreenLayer {
        layer: Layer::Filter { filter, .. },
        ..
    } = &mut mask[1]
    else {
        unreachable!()
    };
    *filter = Filter::ComponentTransfer(Box::new([22; COMPONENT_TRANSFER_TABLE_LEN]));
    plan.ops.extend(mask);
    (canvas, plan)
}
