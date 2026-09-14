use super::*;
use crate::shared::{bounds::Bounds, execution::ExecOp, layer::Layer};
use peniko::{Color, kurbo::Rect};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Recycle,
    Prepare,
    Children,
    Budget(usize),
    Scan,
    Clear(bool),
    Direct(Vec<usize>),
    Recursive(Option<Vec<u32>>),
    Copy,
    Stats,
}

struct Adapter {
    size: (u32, u32),
    plan: Option<Rc<ExecPlan>>,
    damage: Option<DamageTiles>,
    portable: bool,
    fail: Option<&'static str>,
    events: Vec<Event>,
    selected: Vec<u32>,
    expected_batch_ids: Vec<u32>,
}

impl Adapter {
    fn new(plan: &ExecPlan) -> Self {
        Self {
            size: (1024, 1024),
            plan: Some(Rc::new(plan.clone())),
            damage: None,
            portable: false,
            fail: None,
            events: Vec::new(),
            selected: Vec::new(),
            expected_batch_ids: plan.draw_batch_ids.as_ref().clone(),
        }
    }
    fn record(&mut self, event: Event, stage: &'static str) -> Result<(), &'static str> {
        self.events.push(event);
        if self.fail == Some(stage) {
            Err(stage)
        } else {
            Ok(())
        }
    }
    fn partial(&mut self) {
        let mut tiles = DamageTiles::new(self.size);
        tiles.add_bounds(Bounds::new(0, 0, 16, 16));
        self.damage = Some(tiles);
    }
}

impl FrameAdapter for Adapter {
    type Error = &'static str;
    fn recycle_previous_frame(&mut self) {
        self.events.push(Event::Recycle);
    }
    fn active_tiles(&self) -> Option<&DamageTiles> {
        self.damage.as_ref()
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn prepared_plan(&self) -> Option<Rc<ExecPlan>> {
        self.plan.clone()
    }
    fn portable_textures(&self) -> bool {
        self.portable
    }
    fn prepare_frame_resources(&mut self) -> Result<(), Self::Error> {
        self.record(Event::Prepare, "prepare")
    }
    fn encode_vector_images(&mut self) -> Result<(), Self::Error> {
        self.record(Event::Children, "children")
    }
    fn set_initial_root_batch_budget(&mut self, budget: usize) {
        self.events.push(Event::Budget(budget));
    }
    fn scan_scene(&mut self, _: &Canvas) -> Result<(), Self::Error> {
        self.record(Event::Scan, "scan")
    }
    fn clear_root(&mut self, partial: bool) -> Result<(), Self::Error> {
        self.record(Event::Clear(partial), "clear")
    }
    fn active_batch_ids(&mut self, ids: &[u32]) -> Vec<u32> {
        assert_eq!(ids, self.expected_batch_ids);
        self.selected.clone()
    }
    fn execute_direct_root(
        &mut self,
        _: &Canvas,
        _: &ExecPlan,
        ops: &[usize],
    ) -> Result<(), Self::Error> {
        self.record(Event::Direct(ops.to_vec()), "execute")
    }
    fn execute_recursive(
        &mut self,
        _: &Canvas,
        _: &ExecPlan,
        active: Option<&[u32]>,
    ) -> Result<(), Self::Error> {
        self.record(Event::Recursive(active.map(<[u32]>::to_vec)), "execute")
    }
    fn copy_history_to_output(&mut self) -> Result<(), Self::Error> {
        self.record(Event::Copy, "copy")
    }
    fn record_filter_stats(&mut self) {
        self.events.push(Event::Stats);
    }
}

fn fixture(count: u32, recursive: bool) -> (Canvas, ExecPlan) {
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    let mut ops = Vec::new();
    let ids: Vec<u32> = (0..count).map(|i| 100 - i).collect();
    for i in 0..count {
        canvas.push_rect(
            Rect::new(i as f64, 0.0, i as f64 + 1.0, 1.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        ops.push(ExecOp::DrawBatch {
            draws: Rc::new(vec![i as usize]),
            batch_id: ids[i as usize],
            owners: Rc::default(),
            layer_stack: 0..0,
        });
    }
    if recursive {
        ops.push(ExecOp::OffscreenLayer {
            retained_id: None,
            draw: 0,
            layer: Layer::Isolate,
            outer_stack: 0..0,
            children: Vec::new(),
        });
    }
    let plan = ExecPlan {
        ops,
        layer_stack_data: Vec::new(),
        draw_order: Rc::new((0..count).collect()),
        draw_batch_ids: Rc::new(ids.clone()),
        retained_batch_ids: Default::default(),
        layer_stack_locations: Default::default(),
        direct_root_batch_ops: (!recursive).then(|| {
            ids.into_iter()
                .enumerate()
                .map(|(index, id)| (id, index))
                .collect()
        }),
    };
    (canvas, plan)
}

#[test]
fn empty_damage_preserves_required_output_copy_without_preparing_gpu_work() {
    let (canvas, plan) = fixture(1, false);
    for copy in [false, true] {
        let mut adapter = Adapter::new(&plan);
        adapter.damage = Some(DamageTiles::new(adapter.size));
        adapter.plan = None;
        adapter.fail = Some("prepare");
        assert_eq!(encode(&mut adapter, &canvas, copy, true), Ok(()));
        assert_eq!(
            adapter.events,
            if copy {
                vec![Event::Recycle, Event::Copy]
            } else {
                vec![Event::Recycle]
            }
        );
    }
}

#[test]
fn empty_damage_copy_failure_is_reported_without_inventing_render_work() {
    let (canvas, plan) = fixture(1, false);
    let mut adapter = Adapter::new(&plan);
    adapter.damage = Some(DamageTiles::new(adapter.size));
    adapter.plan = None;
    adapter.fail = Some("copy");
    assert_eq!(
        encode(&mut adapter, &canvas, true, true),
        Err(FrameError::Adapter("copy"))
    );
    assert_eq!(adapter.events, vec![Event::Recycle, Event::Copy]);
}

#[test]
fn missing_nonempty_plan_fails_before_target_work_or_history_copy() {
    let (canvas, plan) = fixture(1, false);
    let mut adapter = Adapter::new(&plan);
    adapter.plan = None;
    assert_eq!(
        encode(&mut adapter, &canvas, true, true),
        Err(FrameError::MissingPlan)
    );
    assert_eq!(adapter.events, vec![Event::Recycle]);
}

#[test]
fn large_full_direct_frame_preserves_child_scan_clear_draw_copy_order_and_budget() {
    let (canvas, plan) = fixture(16, false);
    let mut adapter = Adapter::new(&plan);
    encode(&mut adapter, &canvas, true, true).unwrap();
    assert_eq!(
        adapter.events,
        vec![
            Event::Recycle,
            Event::Prepare,
            Event::Children,
            Event::Budget(4),
            Event::Scan,
            Event::Clear(false),
            Event::Direct((0..16).collect()),
            Event::Copy,
            Event::Stats,
        ]
    );
}

#[test]
fn portable_partial_small_and_disallowed_frames_do_not_enable_early_root_submit() {
    let (canvas, plan) = fixture(16, false);
    for mode in 0..4 {
        let mut adapter = Adapter::new(&plan);
        if mode == 0 {
            adapter.portable = true;
        }
        if mode == 1 {
            adapter.partial();
        }
        if mode == 2 {
            adapter.size = (512, 512);
        }
        encode(&mut adapter, &canvas, false, mode != 3).unwrap();
        assert!(!adapter.events.iter().any(|e| matches!(e, Event::Budget(_))));
        assert!(adapter.events.contains(&Event::Clear(mode == 1)));
        assert!(!adapter.events.contains(&Event::Copy));
    }
}

#[test]
fn retained_batch_selection_uses_stable_membership_but_executes_in_painter_order() {
    let (mut canvas, plan) = fixture(3, false);
    canvas.stable_batch_ids = Some(vec![700, 800, 900]);
    let mut adapter = Adapter::new(&plan);
    adapter.expected_batch_ids = vec![700, 800, 900];
    adapter.selected = vec![98, 100];
    adapter.partial();
    encode(&mut adapter, &canvas, false, true).unwrap();
    assert!(adapter.events.contains(&Event::Direct(vec![0, 2])));
    assert!(adapter.events.contains(&Event::Clear(true)));
}

#[test]
fn recursive_frame_receives_root_selection_without_skipping_its_layer_execution() {
    let (canvas, plan) = fixture(1, true);
    let mut adapter = Adapter::new(&plan);
    adapter.partial();
    encode(&mut adapter, &canvas, true, true).unwrap();
    assert!(adapter.events.contains(&Event::Recursive(Some(Vec::new()))));
    assert_eq!(
        &adapter.events[adapter.events.len() - 2..],
        &[Event::Copy, Event::Stats]
    );
}

#[test]
fn preparation_failures_stop_before_any_dependent_work() {
    let (canvas, plan) = fixture(1, false);
    let stages = ["prepare", "children", "scan", "clear"];
    let prefix = [
        Event::Recycle,
        Event::Prepare,
        Event::Children,
        Event::Scan,
        Event::Clear(false),
    ];
    for (index, stage) in stages.into_iter().enumerate() {
        let mut adapter = Adapter::new(&plan);
        adapter.fail = Some(stage);
        assert_eq!(
            encode(&mut adapter, &canvas, true, true),
            Err(FrameError::Adapter(stage))
        );
        assert_eq!(adapter.events, prefix[..index + 2]);
    }
}

#[test]
fn either_execution_failure_records_stats_but_never_copies_failed_history() {
    for recursive in [false, true] {
        let (canvas, plan) = fixture(1, recursive);
        let mut adapter = Adapter::new(&plan);
        adapter.fail = Some("execute");
        assert_eq!(
            encode(&mut adapter, &canvas, true, true),
            Err(FrameError::Adapter("execute"))
        );
        assert_eq!(adapter.events.last(), Some(&Event::Stats));
        assert!(!adapter.events.contains(&Event::Copy));
    }
}

#[test]
fn output_copy_failure_keeps_recorded_execution_statistics_and_returns_failure() {
    let (canvas, plan) = fixture(1, false);
    let mut adapter = Adapter::new(&plan);
    adapter.fail = Some("copy");
    assert_eq!(
        encode(&mut adapter, &canvas, true, true),
        Err(FrameError::Adapter("copy"))
    );
    assert_eq!(
        &adapter.events[adapter.events.len() - 2..],
        &[Event::Copy, Event::Stats]
    );
}
