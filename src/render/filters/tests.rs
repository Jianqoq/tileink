use super::*;
use crate::render::{
    retained_surfaces::RetainedSurfaceMeta,
    surfaces::{
        SurfaceAdapter,
        test_support::{Adapter, Allocation, Event, fixture, id},
    },
};
use crate::shared::sdf::rect::Radius;
use peniko::kurbo::Rect;

pub(crate) struct SavedFilterState {
    context: Option<((u32, u32), (i32, i32))>,
    targets: Vec<Option<Allocation>>,
    damage: Option<DamageTiles>,
}
impl FilterAdapter for Adapter {
    type LocalState = SavedFilterState;
    fn filter_candidates(&self, _: Bounds, _: &ExecPlan) -> Vec<u32> {
        self.candidate_queries.set(self.candidate_queries.get() + 1);
        self.candidate_draws.clone()
    }
    fn begin_filter_scene(
        &mut self,
        canvas: &Canvas,
        _: &ExecPlan,
        _: &Filter,
        _: usize,
        origin: (i32, i32),
        reuse_root: bool,
    ) -> Result<SavedFilterState, Self::Error> {
        let size = (canvas.physical_width(), canvas.physical_height());
        self.record(Event::BeginFilter(size, origin, reuse_root), "begin-filter")?;
        let damage = self.retained.active_tiles().cloned();
        let state = SavedFilterState {
            context: self.filter_context,
            targets: std::mem::take(&mut self.targets),
            damage,
        };
        self.filter_context = Some((size, origin));
        if !reuse_root {
            self.retained.set_active_tiles(None);
        }
        Ok(state)
    }
    fn end_filter_scene(&mut self, state: SavedFilterState) {
        for slot in 0..self.targets.len() {
            if self.targets[slot].is_some() {
                self.release_scratch(RenderTargetId::Scratch(slot));
            }
        }
        self.targets = state.targets;
        self.filter_context = state.context;
        self.retained.set_active_tiles(state.damage);
        self.filter_ends += 1;
        self.events.push(Event::EndFilter);
    }
    fn prepare_filter_source_work(&mut self) -> Result<(), Self::Error> {
        self.record(Event::SourceWork, "source-work")
    }
    fn scan_filter_scene(&mut self, _: &Canvas) -> Result<(), Self::Error> {
        self.record(Event::ScanFilter, "scan")
    }
    fn composite_filter(
        &mut self,
        target: RenderTargetId,
        source: &Allocation,
        placement: FilterPlacement,
        stack: Range<usize>,
    ) -> Result<(), Self::Error> {
        assert!(
            self.filter_context.is_none(),
            "compositing must use the restored parent context"
        );
        self.filter_placements.push((target, source.0, placement));
        self.composite_cached(target, source, None, placement.bounds, stack, None)
    }
}
fn meta(adapter: &Adapter) -> RetainedSurfaceMeta {
    adapter
        .retained
        .surface_meta(
            id(),
            RetainedSurfaceKind::Filter,
            (32, 32),
            (0, 0),
            Bounds::canvas(32, 32),
        )
        .unwrap()
}
fn cache(adapter: &mut Adapter) {
    let meta = meta(adapter);
    adapter.retained.cache_surface(
        Some(id()),
        Some(meta),
        Allocation(20),
        Some(Allocation(30)),
        None,
    );
}
fn cache_ids(adapter: &mut Adapter) -> Option<(u32, u32)> {
    let meta = meta(adapter);
    adapter
        .retained
        .take_matching_surface(Some(id()), Some(meta))
        .map(|(_, surface)| (surface.primary.0, surface.secondary.unwrap().0))
}
fn render(
    adapter: &mut Adapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    retained: bool,
    bounds: Bounds,
    cursors: &mut FilterCursors,
) -> Result<(), &'static str> {
    let filter = Filter::ComponentTransfer(Box::new(
        [22; crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_LEN],
    ));
    let region = Region::Rect {
        rect: Rect::new(
            bounds.x0 as f64,
            bounds.y0 as f64,
            bounds.x1 as f64,
            bounds.y1 as f64,
        ),
        radius: Radius::ZERO,
    };
    execute(
        adapter,
        canvas,
        plan,
        FilterLayer {
            retained_id: retained.then(id),
            filter: &filter,
            region: &region,
            stack: 2..4,
            children: &plan.ops,
        },
        RenderTargetId::Main,
        cursors,
    )
}
fn dirty_tile(adapter: &mut Adapter) {
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(16, 16, 32, 32));
    adapter.retained.set_active_tiles(Some(damage));
}

#[test]
fn uncached_filter_restores_parent_pool_and_processes_children_before_parent() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    adapter.targets.push(Some(Allocation(900)));
    let mut cursors = FilterCursors::default();
    render(
        &mut adapter,
        &canvas,
        &plan,
        false,
        Bounds::canvas(32, 32),
        &mut cursors,
    )
    .unwrap();
    assert_eq!(adapter.filters, [11, 22]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(adapter.targets.len(), 1);
    assert_eq!(adapter.targets[0].as_ref().unwrap().0, 900);
    assert_eq!(adapter.filter_ends, 1);
    assert_eq!(adapter.origin(), (4, 8));
    assert_eq!(adapter.candidate_queries.get(), 0);
}

#[test]
fn clean_filter_cache_advances_resources_without_entering_local_context() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    cache(&mut adapter);
    adapter
        .retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    let mut cursors = FilterCursors::default();
    render(
        &mut adapter,
        &canvas,
        &plan,
        true,
        Bounds::canvas(32, 32),
        &mut cursors,
    )
    .unwrap();
    assert_eq!(adapter.events, [Event::Cached(20, None, None)]);
    assert_eq!(adapter.filter_ends, 0);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(cache_ids(&mut adapter), Some((20, 30)));
}

#[test]
fn partial_filter_keeps_both_history_images_and_restores_parent_damage() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    cache(&mut adapter);
    dirty_tile(&mut adapter);
    render(
        &mut adapter,
        &canvas,
        &plan,
        true,
        Bounds::canvas(32, 32),
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert!(
        adapter
            .events
            .contains(&Event::Install(RenderTargetId::Scratch(0), 30))
    );
    assert!(
        adapter
            .events
            .contains(&Event::Install(RenderTargetId::Scratch(1), 20))
    );
    assert_eq!(cache_ids(&mut adapter), Some((20, 30)));
    assert!(adapter.targets.is_empty());
    assert_eq!(
        adapter
            .retained
            .active_tiles()
            .unwrap()
            .bounds_union((32, 32)),
        Some(Bounds::new(16, 16, 32, 32))
    );
    assert_eq!(adapter.retained.stats().rerendered_offscreen_tiles, 1);
}

#[test]
fn every_recording_error_restores_context_drops_invalid_cache_and_allows_retry() {
    for (cached, stage, skip) in [
        (false, "clear", 0),
        (false, "scan", 0),
        (false, "children", 0),
        (false, "filter", 0),
        (false, "copy", 0),
        (false, "apply", 0),
        (true, "source-work", 0),
        (true, "clear", 0),
        (true, "copy", 0),
        (true, "apply", 0),
        (true, "output-work", 0),
        (true, "copy", 1),
    ] {
        let (canvas, plan) = fixture();
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
        if cached {
            cache(&mut adapter);
            dirty_tile(&mut adapter);
        }
        adapter.fail = Some(stage);
        adapter.skip_failures = skip;
        assert_eq!(
            render(
                &mut adapter,
                &canvas,
                &plan,
                true,
                Bounds::canvas(32, 32),
                &mut FilterCursors::default()
            ),
            Err(stage),
            "{cached} {stage} {skip}"
        );
        assert_eq!(adapter.filter_ends, 1);
        assert!(adapter.filter_context.is_none());
        assert!(adapter.targets.is_empty());
        assert_eq!(cache_ids(&mut adapter), None);
        adapter.fail = None;
        render(
            &mut adapter,
            &canvas,
            &plan,
            true,
            Bounds::canvas(32, 32),
            &mut FilterCursors::default(),
        )
        .unwrap();
        assert_eq!(adapter.filter_ends, 2);
        assert!(cache_ids(&mut adapter).is_some());
    }
}

#[test]
fn entering_or_allocating_failure_preserves_the_parent_context() {
    for stage in ["begin-filter", "scratch"] {
        let (canvas, plan) = fixture();
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
        adapter.targets.push(Some(Allocation(900)));
        if stage == "begin-filter" {
            adapter.fail = Some(stage);
        } else {
            adapter.scratch_limit = 0;
        }
        assert_eq!(
            render(
                &mut adapter,
                &canvas,
                &plan,
                false,
                Bounds::canvas(32, 32),
                &mut FilterCursors::default()
            ),
            Err(stage)
        );
        assert!(adapter.filter_context.is_none());
        assert_eq!(adapter.targets[0].as_ref().unwrap().0, 900);
        assert_eq!(adapter.filter_ends, usize::from(stage == "scratch"));
    }
}

#[test]
fn composite_failure_preserves_successfully_recorded_filter_history() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    adapter.fail = Some("composite");
    assert_eq!(
        render(
            &mut adapter,
            &canvas,
            &plan,
            true,
            Bounds::canvas(32, 32),
            &mut FilterCursors::default()
        ),
        Err("composite")
    );
    assert_eq!(cache_ids(&mut adapter), Some((100, 101)));
    assert!(adapter.targets.is_empty());
    assert_eq!(adapter.filter_ends, 1);
}

#[test]
fn empty_filter_advances_both_resource_sequences_without_gpu_work() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    let mut cursors = FilterCursors::default();
    render(
        &mut adapter,
        &canvas,
        &plan,
        false,
        Bounds::new(64, 64, 96, 96),
        &mut cursors,
    )
    .unwrap();
    assert!(adapter.events.is_empty());
    assert_eq!(cursors.next_transfer_index(), 2);
}

#[test]
fn cropped_filter_uses_local_resources_and_restores_parent_size_and_origin() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    let mut cursors = FilterCursors::default();
    render(
        &mut adapter,
        &canvas,
        &plan,
        false,
        Bounds::new(8, 8, 24, 24),
        &mut cursors,
    )
    .unwrap();
    assert_eq!(adapter.candidate_queries.get(), 1);
    assert!(
        adapter
            .events
            .contains(&Event::BeginFilter((16, 16), (12, 16), false))
    );
    assert_eq!(adapter.size(), (32, 32));
    assert_eq!(adapter.origin(), (4, 8));
    assert_eq!(cursors.next_transfer_index(), 2);
}

#[test]
fn edge_filter_keeps_expanded_surface_origin_separate_from_visible_output() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    let region = Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO);
    execute(
        &mut adapter,
        &canvas,
        &plan,
        FilterLayer {
            retained_id: None,
            filter: &Filter::Offset { dx: 2.0, dy: 1.0 },
            region: &region,
            stack: 2..4,
            children: &plan.ops,
        },
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
    .unwrap();
    assert_eq!(
        adapter.filter_placements,
        [(
            RenderTargetId::Main,
            100,
            FilterPlacement {
                size: (36, 36),
                origin: (-2, -2),
                bounds: Bounds::canvas(32, 32),
            }
        )]
    );
    assert!(
        adapter
            .events
            .contains(&Event::BeginFilter((36, 36), (2, 6), false))
    );
    assert_eq!(adapter.origin(), (4, 8));
}

mod cursors;
