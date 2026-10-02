use super::*;
use crate::render::incremental::IncrementalRenderStats;
use crate::shared::layer::{mask::MaskKind, region::Region};
use std::rc::Rc;

#[derive(Debug, PartialEq)]
enum Event {
    Draw(u32, RenderTargetId),
    Group(usize, u32),
    Mask(Option<u32>),
}

#[derive(Default)]
struct TraceAdapter {
    events: Vec<Event>,
    stats: IncrementalRenderStats,
    batch: u32,
    fail_draw: Option<u32>,
    fail_layer: bool,
}

impl DrawBatchAdapter for TraceAdapter {
    type Error = &'static str;
    fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        &mut self.stats
    }
    fn begin_root_batch(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn coarse(&mut self, batches: Range<u32>, _: Range<u32>) -> Result<(), Self::Error> {
        self.batch = batches.start;
        Ok(())
    }
    fn fine(&mut self, target: RenderTargetId) -> Result<(), Self::Error> {
        self.events.push(Event::Draw(self.batch, target));
        if self.fail_draw == Some(self.batch) {
            Err("draw")
        } else {
            Ok(())
        }
    }
}

impl OperationAdapter for TraceAdapter {
    fn offscreen(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Offscreen<'_>,
        _: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        assert!(matches!(op.layer, Layer::Isolate));
        assert_eq!(op.outer_stack, 2..4);
        assert!(op.retained_id.is_none());
        self.events
            .push(Event::Group(op.draw, cursors.next_transfer_index()));
        if self.fail_layer {
            return Err("layer");
        }
        // Offscreen contents have a local damage decision. Root active IDs must
        // not silently skip the contents needed to rebuild the local image.
        execute_ops(
            self,
            canvas,
            plan,
            op.children,
            RenderTargetId::Scratch(0),
            cursors,
            None,
        )
    }
    fn mask(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Masked<'_>,
        _: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error> {
        assert_eq!(op.outer_stack, 4..6);
        assert!(op.retained_id.is_none());
        assert_eq!(op.layer.kind, MaskKind::Alpha);
        self.events
            .push(Event::Mask(cursors.next_path_index(&op.layer.region)));
        if self.fail_layer {
            return Err("mask");
        }
        execute_ops(
            self,
            canvas,
            plan,
            op.content,
            RenderTargetId::Scratch(1),
            cursors,
            None,
        )?;
        execute_ops(
            self,
            canvas,
            plan,
            op.mask,
            RenderTargetId::Scratch(2),
            cursors,
            None,
        )
    }
}

fn draw(id: u32) -> ExecOp {
    ExecOp::DrawBatch {
        draws: Rc::new(vec![id as usize]),
        batch_id: id,
        owners: Rc::default(),
        layer_stack: 0..0,
    }
}

fn group(children: Vec<ExecOp>) -> ExecOp {
    ExecOp::OffscreenLayer {
        retained_id: None,
        draw: 17,
        layer: Layer::Isolate,
        outer_stack: 2..4,
        children,
    }
}

fn mask(content: Vec<ExecOp>, mask: Vec<ExecOp>) -> ExecOp {
    ExecOp::OffscreenMaskLayer {
        retained_id: None,
        layer: Mask {
            region: Region::Path {
                path: peniko::kurbo::BezPath::new(),
                transform: peniko::kurbo::Affine::IDENTITY,
                tolerance: 0.1,
            },
            kind: MaskKind::Alpha,
        },
        outer_stack: 4..6,
        content,
        mask,
    }
}

fn plan(ops: Vec<ExecOp>) -> ExecPlan {
    ExecPlan {
        ops,
        layer_stack_data: Vec::new(),
        draw_order: Rc::default(),
        draw_batch_ids: Rc::default(),
        retained_batch_ids: Default::default(),
        layer_stack_locations: Default::default(),
        direct_root_batch_ops: None,
    }
}

fn run(
    adapter: &mut TraceAdapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    active: Option<&[u32]>,
) -> Result<(), &'static str> {
    execute_ops(
        adapter,
        canvas,
        plan,
        &plan.ops,
        RenderTargetId::Main,
        &mut FilterCursors::default(),
        active,
    )
}

#[test]
fn root_selection_preserves_painter_order_and_nested_target_contents() {
    let canvas = Canvas::new(17, 19, 1.0);
    let plan = plan(vec![
        draw(8),
        draw(1),
        group(vec![draw(2)]),
        mask(vec![draw(3)], vec![draw(4)]),
        draw(0),
    ]);
    let mut adapter = TraceAdapter::default();
    run(&mut adapter, &canvas, &plan, Some(&[0, 8])).unwrap();
    assert_eq!(
        adapter.events,
        [
            Event::Draw(8, RenderTargetId::Main),
            Event::Group(17, 0),
            Event::Draw(2, RenderTargetId::Scratch(0)),
            Event::Mask(Some(0)),
            Event::Draw(3, RenderTargetId::Scratch(1)),
            Event::Draw(4, RenderTargetId::Scratch(2)),
            Event::Draw(0, RenderTargetId::Main)
        ]
    );
    assert_eq!(adapter.stats.draw_batches, 5);
    assert_eq!(adapter.stats.root_draw_batches, 2);
}

#[test]
fn empty_root_selection_still_visits_layers_and_shares_resource_cursors() {
    let canvas = Canvas::new(17, 19, 1.0);
    let plan = plan(vec![
        draw(0),
        group(vec![]),
        mask(vec![], vec![]),
        group(vec![]),
        mask(vec![], vec![]),
    ]);
    let mut adapter = TraceAdapter::default();
    run(&mut adapter, &canvas, &plan, Some(&[])).unwrap();
    assert_eq!(
        adapter.events,
        [
            Event::Group(17, 0),
            Event::Mask(Some(0)),
            Event::Group(17, 1),
            Event::Mask(Some(1))
        ]
    );
    assert_eq!(adapter.stats.draw_batches, 0);
}

#[test]
fn dead_batches_and_fused_stack_markers_do_not_issue_extra_work() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.stable_batch_counts = Some(vec![1, 0, 1]);
    let plan = plan(vec![
        ExecOp::BeginClip,
        ExecOp::BeginOpacity,
        ExecOp::BeginBlend,
        draw(2),
        draw(1),
        draw(0),
        ExecOp::EndBlend,
        ExecOp::EndOpacity,
        ExecOp::EndClip,
    ]);
    let mut adapter = TraceAdapter::default();
    run(&mut adapter, &canvas, &plan, None).unwrap();
    assert_eq!(
        adapter.events,
        [
            Event::Draw(2, RenderTargetId::Main),
            Event::Draw(0, RenderTargetId::Main)
        ]
    );
}

#[test]
fn nested_failure_reaches_the_frame_owner_before_any_later_work() {
    let canvas = Canvas::new(17, 19, 1.0);
    for nested in [
        group(vec![draw(2), draw(3)]),
        mask(vec![draw(2)], vec![draw(3)]),
    ] {
        let plan = plan(vec![draw(0), nested, draw(4)]);
        let mut adapter = TraceAdapter {
            fail_draw: Some(2),
            ..Default::default()
        };
        assert_eq!(run(&mut adapter, &canvas, &plan, None), Err("draw"));
        assert_eq!(adapter.stats.draw_batches, 2);
        assert!(
            !adapter
                .events
                .iter()
                .any(|event| matches!(event, Event::Draw(3 | 4, _)))
        );
    }
}

#[test]
fn layer_rejection_stops_before_children_and_succeeding_siblings() {
    let canvas = Canvas::new(17, 19, 1.0);
    for (nested, error) in [
        (group(vec![draw(2)]), "layer"),
        (mask(vec![draw(2)], vec![draw(3)]), "mask"),
    ] {
        let plan = plan(vec![nested, draw(4)]);
        let mut adapter = TraceAdapter {
            fail_layer: true,
            ..Default::default()
        };
        assert_eq!(run(&mut adapter, &canvas, &plan, None), Err(error));
        assert_eq!(adapter.events.len(), 1);
        assert_eq!(adapter.stats.draw_batches, 0);
    }
}

#[test]
fn empty_plan_is_success_without_issuing_gpu_work() {
    let mut adapter = TraceAdapter::default();
    run(&mut adapter, &Canvas::new(1, 1, 1.0), &plan(vec![]), None).unwrap();
    assert!(adapter.events.is_empty());
}
