use super::*;
use crate::{Filter, Mask, MaskKind, Radius, Region};
use peniko::kurbo::Rect;
use std::rc::Rc;

#[test]
fn an_empty_or_unbudgeted_frame_never_requests_an_early_submission() {
    let mut schedule = BatchSchedule::default();
    assert_eq!(schedule.submissions(), 0);
    for _ in 0..32 {
        assert!(!schedule.begin_root_batch());
    }
}

#[test]
fn submit_only_when_a_real_successor_root_batch_arrives() {
    let mut schedule = BatchSchedule::default();
    schedule.set_initial_root_batch_budget(2);
    assert!(!schedule.begin_root_batch());
    assert!(!schedule.begin_root_batch());
    assert_eq!(schedule.submissions(), 0);
    assert!(schedule.begin_root_batch());
    schedule.record_submission();
    for _ in 0..16 {
        assert!(!schedule.begin_root_batch());
    }
    assert_eq!(schedule.submissions(), 1);
}

#[test]
fn a_short_frame_finishes_without_an_extra_latency_submission() {
    let mut schedule = BatchSchedule::default();
    schedule.set_initial_root_batch_budget(2);
    assert!(!schedule.begin_root_batch());
    assert!(!schedule.begin_root_batch());
    schedule.record_submission();
    assert_eq!(schedule.submissions(), 1);
}

#[test]
fn uniform_rollover_suppresses_a_second_early_submission() {
    for consumed in 0..=2 {
        let mut schedule = BatchSchedule::default();
        schedule.set_initial_root_batch_budget(2);
        for _ in 0..consumed {
            assert!(!schedule.begin_root_batch());
        }
        schedule.record_submission();
        for _ in 0..8 {
            assert!(!schedule.begin_root_batch());
        }
        assert_eq!(schedule.submissions(), 1);
    }
}

#[test]
fn a_submit_request_without_gpu_work_is_not_counted_as_a_submission() {
    let mut schedule = BatchSchedule::default();
    schedule.set_initial_root_batch_budget(0);
    assert!(schedule.begin_root_batch());
    assert!(!schedule.begin_root_batch());
    assert_eq!(schedule.submissions(), 0);
}

#[test]
fn immediate_empty_batches_do_not_consume_the_root_budget() {
    let canvas = Canvas::new(17, 19, 1.0);
    assert!(!draw_batch_is_live(&canvas, &[], 0));
    assert!(draw_batch_is_live(&canvas, &[3], 0));
}

#[test]
fn retained_live_counts_override_stale_draw_lists() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.stable_batch_counts = Some(vec![2, 0]);
    assert!(draw_batch_is_live(&canvas, &[], 0));
    assert!(!draw_batch_is_live(&canvas, &[3], 1));
    assert!(!draw_batch_is_live(&canvas, &[3], 2));
}

fn draw(id: u32) -> ExecOp {
    ExecOp::DrawBatch {
        draws: Rc::new(vec![id as usize]),
        batch_id: id,
        owners: Rc::new(Vec::new()),
        layer_stack: 0..0,
    }
}

fn region(rect: Rect) -> Region {
    Region::Rect {
        rect,
        radius: Radius::ZERO,
    }
}

fn offscreen(layer: Layer, children: Vec<ExecOp>) -> ExecOp {
    ExecOp::OffscreenLayer {
        retained_id: None,
        draw: 0,
        layer,
        outer_stack: 0..0,
        children,
    }
}

#[test]
fn root_budget_follows_backdrop_foreground_but_excludes_scratch_work() {
    let canvas = Canvas::new(17, 19, 1.0);
    let full = || region(Rect::new(0.0, 0.0, 17.0, 19.0));
    let backdrop = || Layer::Backdrop {
        filter: Filter::Brightness(1.0),
        sample_region: full(),
    };
    let ops = vec![
        ExecOp::BeginClip,
        draw(0),
        ExecOp::EndClip,
        offscreen(Layer::Isolate, vec![draw(1)]),
        offscreen(
            backdrop(),
            vec![draw(2), offscreen(backdrop(), vec![draw(3)])],
        ),
        offscreen(
            Layer::Backdrop {
                filter: Filter::Brightness(1.0),
                sample_region: region(Rect::new(30.0, 30.0, 40.0, 40.0)),
            },
            vec![draw(4)],
        ),
        ExecOp::OffscreenMaskLayer {
            retained_id: None,
            layer: Mask {
                region: full(),
                kind: MaskKind::Alpha,
            },
            outer_stack: 0..0,
            content: vec![draw(5)],
            mask: vec![draw(6)],
        },
    ];
    assert_eq!(root_draw_batch_count(&canvas, &ops), 4);
}

#[test]
fn early_submission_eligibility_keeps_the_full_frame_and_texture_mode_contract() {
    let canvas = Canvas::new(1024, 1024, 1.0);
    let ops: Vec<_> = (0..16).map(draw).collect();
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops, (1024, 1024), true, false, false),
        Some(4)
    );
    for (allow, partial, portable) in [
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        assert_eq!(
            initial_root_batch_budget(&canvas, &ops, (1024, 1024), allow, partial, portable),
            None
        );
    }
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops, (1024, 1023), true, false, false),
        None
    );
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops[..15], (1024, 1024), true, false, false),
        None
    );
}

#[test]
fn early_submission_budget_counts_only_live_root_batches_and_handles_large_surfaces() {
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    let ops: Vec<_> = (0..64).map(draw).collect();
    canvas.stable_batch_counts = Some((0..64).map(|i| u32::from(i % 4 == 0)).collect());
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops, (u32::MAX, u32::MAX), true, false, false),
        Some(4)
    );
    canvas.stable_batch_counts.as_mut().unwrap()[0] = 0;
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops, (1024, 1024), true, false, false),
        None
    );
}

// The backdrop effect may be empty while its foreground still contains every
// live Main batch. Preserve the existing overlap policy for that real work.
#[test]
fn empty_backdrop_children_keep_the_first_quarter_submission_budget() {
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    canvas.stable_batch_counts = Some((0..32).map(|i| u32::from(i % 2 == 0)).collect());
    let backdrop = || Layer::Backdrop {
        filter: Filter::Brightness(1.0),
        sample_region: region(Rect::new(2048.0, 2048.0, 2064.0, 2064.0)),
    };
    let ops = vec![offscreen(
        backdrop(),
        vec![offscreen(backdrop(), (0..32).map(draw).collect())],
    )];
    let budget = initial_root_batch_budget(&canvas, &ops, (1024, 1024), true, false, false);
    assert_eq!(
        budget,
        Some(4),
        "empty backdrop foreground still executes sixteen live Main batches"
    );
    let mut schedule = BatchSchedule::default();
    schedule.set_initial_root_batch_budget(budget.unwrap());
    let mut submitted_before = Vec::new();
    for batch in 0..16 {
        if schedule.begin_root_batch() {
            submitted_before.push(batch);
            schedule.record_submission();
        }
    }
    assert_eq!(submitted_before, vec![4]);
    schedule.record_submission();
    assert_eq!(schedule.submissions(), 2);
    canvas.stable_batch_counts.as_mut().unwrap()[0] = 0;
    assert_eq!(
        initial_root_batch_budget(&canvas, &ops, (1024, 1024), true, false, false),
        None
    );
}
