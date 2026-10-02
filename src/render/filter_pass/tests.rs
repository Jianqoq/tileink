use super::*;
use crate::render::retained_surfaces::RetainedSurfaceKind;
use crate::render::surfaces::{
    SurfaceAdapter,
    test_support::{Adapter, Event, fixture},
};

fn adapter() -> Adapter {
    let (canvas, _) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    assert_eq!(
        adapter.acquire_scratch().unwrap(),
        RenderTargetId::Scratch(0)
    );
    assert_eq!(
        adapter.acquire_scratch().unwrap(),
        RenderTargetId::Scratch(1)
    );
    adapter.events.clear();
    adapter
}
fn partial() -> FilterHistory {
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(16, 16, 32, 32));
    FilterHistory::Partial {
        filtered: RenderTargetId::Scratch(1),
        update: Bounds::new(16, 16, 32, 32),
        damage,
    }
}
fn run(
    adapter: &mut Adapter,
    history: FilterHistory,
    cursors: &mut FilterCursors,
) -> Result<(), &'static str> {
    execute(
        adapter,
        RenderTargetId::Scratch(0),
        Bounds::canvas(32, 32),
        &Filter::Offset { dx: 2.0, dy: 0.0 },
        history,
        cursors,
    )
}

#[test]
fn full_render_captures_unfiltered_history_before_applying_filter() {
    let mut adapter = adapter();
    run(
        &mut adapter,
        FilterHistory::Capture(RenderTargetId::Scratch(1)),
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert_eq!(
        adapter.events,
        vec![
            Event::Copy(
                RenderTargetId::Scratch(0),
                RenderTargetId::Scratch(1),
                Bounds::canvas(32, 32)
            ),
            Event::FilterPass(RenderTargetId::Scratch(0), Bounds::canvas(32, 32))
        ]
    );
}

#[test]
fn uncached_filter_has_no_history_copy_and_consumes_its_resources_once() {
    let mut adapter = adapter();
    let mut cursors = FilterCursors::default();
    assert_eq!(cursors.next_transfer_index(), 0);
    execute(
        &mut adapter,
        RenderTargetId::Scratch(0),
        Bounds::canvas(32, 32),
        &Filter::ComponentTransfer(Box::new(
            [11; crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_LEN],
        )),
        FilterHistory::None,
        &mut cursors,
    )
    .unwrap();
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(
        adapter.events,
        vec![Event::FilterPass(
            RenderTargetId::Scratch(0),
            Bounds::canvas(32, 32)
        )]
    );
}

#[test]
fn partial_filter_processes_dependency_halo_but_updates_only_output_damage() {
    let mut adapter = adapter();
    run(&mut adapter, partial(), &mut FilterCursors::default()).unwrap();
    let source = RenderTargetId::Scratch(0);
    let filtered = RenderTargetId::Scratch(1);
    let temp = RenderTargetId::Scratch(2);
    assert_eq!(
        adapter.events,
        vec![
            Event::Acquire(temp),
            Event::Copy(source, temp, Bounds::new(14, 14, 32, 32)),
            Event::FilterPass(temp, Bounds::new(14, 14, 32, 32)),
            Event::OutputDamage,
            Event::Copy(temp, filtered, Bounds::new(16, 16, 32, 32)),
            Event::Release(temp)
        ]
    );
    let damage = adapter.retained.active_tiles().unwrap();
    assert_eq!(
        damage.bounds_union((32, 32)),
        Some(Bounds::new(16, 16, 32, 32))
    );
    assert!(adapter.targets[2].is_none());
}

#[test]
fn partial_failure_releases_temp_and_preserves_both_caller_owned_history_slots() {
    for (stage, skip) in [("copy", 0), ("apply", 0), ("output-work", 0), ("copy", 1)] {
        let mut adapter = adapter();
        adapter.fail = Some(stage);
        adapter.skip_failures = skip;
        assert_eq!(
            run(&mut adapter, partial(), &mut FilterCursors::default()),
            Err(stage)
        );
        assert!(adapter.targets[0].is_some() && adapter.targets[1].is_some());
        assert!(adapter.targets[2].is_none());
        assert_eq!(
            adapter
                .events
                .iter()
                .filter(|event| **event == Event::Release(RenderTargetId::Scratch(2)))
                .count(),
            1
        );
        adapter.fail = None;
        run(&mut adapter, partial(), &mut FilterCursors::default()).unwrap();
        assert!(adapter.targets[2].is_none());
    }
}

#[test]
fn allocation_failure_does_not_touch_history_or_advance_filter_resources() {
    let mut adapter = adapter();
    adapter.scratch_limit = 2;
    let mut cursors = FilterCursors::default();
    assert_eq!(run(&mut adapter, partial(), &mut cursors), Err("scratch"));
    assert!(adapter.events.is_empty());
    assert_eq!(cursors.next_transfer_index(), 0);
    assert!(adapter.targets.iter().all(Option::is_some));
}

#[test]
fn failed_history_capture_does_not_apply_the_filter() {
    let mut adapter = adapter();
    adapter.fail = Some("copy");
    assert_eq!(
        run(
            &mut adapter,
            FilterHistory::Capture(RenderTargetId::Scratch(1)),
            &mut FilterCursors::default()
        ),
        Err("copy")
    );
    assert_eq!(adapter.events.len(), 1);
    assert!(matches!(adapter.events[0], Event::Copy(..)));
}

#[test]
fn partial_erosion_reads_its_neighbour_halo_before_updating_only_output_damage() {
    let mut adapter = adapter();
    let filter = Filter::Morphology {
        radius_x: 1.0,
        radius_y: 0.0,
        operator: crate::shared::layer::filter::MorphologyOperator::Erode,
    };
    execute(
        &mut adapter,
        RenderTargetId::Scratch(0),
        Bounds::canvas(32, 32),
        &filter,
        partial(),
        &mut FilterCursors::default(),
    )
    .unwrap();
    let temp = RenderTargetId::Scratch(2);
    assert!(
        adapter.events.contains(&Event::Copy(
            RenderTargetId::Scratch(0),
            temp,
            Bounds::new(15, 15, 32, 32)
        )),
        "erosion reads neighbouring pixels even though its output region does not expand"
    );
    assert!(
        adapter
            .events
            .contains(&Event::FilterPass(temp, Bounds::new(15, 15, 32, 32)))
    );
    assert!(adapter.events.contains(&Event::Copy(
        temp,
        RenderTargetId::Scratch(1),
        Bounds::new(16, 16, 32, 32)
    )));
    assert_eq!(
        adapter
            .retained
            .active_tiles()
            .unwrap()
            .bounds_union((32, 32)),
        Some(Bounds::new(16, 16, 32, 32))
    );
    assert!(adapter.targets[2].is_none());
}
