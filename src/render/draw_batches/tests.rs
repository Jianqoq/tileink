use super::*;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Coarse(u32),
    Fine(u32),
}

struct MemoryAdapter {
    main: [u32; 4],
    scratch: [u32; 4],
    partial: bool,
    stats: IncrementalRenderStats,
    coarse_batches: Vec<u32>,
    last_batch: u32,
    fail: Option<Failure>,
}

impl MemoryAdapter {
    fn new(partial: bool) -> Self {
        Self {
            main: [1, 2, 3, 4],
            scratch: [7, 8, 9, 10],
            partial,
            stats: IncrementalRenderStats::default(),
            coarse_batches: Vec::new(),
            last_batch: 0,
            fail: None,
        }
    }

    // Noncommutative painter composition exposes order and stale-source errors.
    // Sparse dispatch leaves the other pixels untouched, just as a dirty tile does.
    fn paint(&self, source: [u32; 4], target: &mut [u32; 4]) -> Result<(), Failure> {
        for pixel in 0..4 {
            if !self.partial || pixel % 2 == 0 {
                target[pixel] = source[pixel] * 10 + self.last_batch + 1;
                if self.fail == Some(Failure::Fine(self.last_batch)) {
                    return Err(Failure::Fine(self.last_batch));
                }
            }
        }
        Ok(())
    }
}

impl DrawBatchAdapter for MemoryAdapter {
    type Error = Failure;

    fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        &mut self.stats
    }

    fn coarse(&mut self, batches: Range<u32>, _: Range<u32>) -> Result<(), Failure> {
        let batch_id = batches.start;
        assert_eq!(batches.end, batch_id.saturating_add(1));
        self.coarse_batches.push(batch_id);
        self.last_batch = batch_id;
        if self.fail == Some(Failure::Coarse(batch_id)) {
            Err(Failure::Coarse(batch_id))
        } else {
            Ok(())
        }
    }

    fn fine(&mut self, target: RenderTargetId) -> Result<(), Failure> {
        let mut pixels = match target {
            RenderTargetId::Main => self.main,
            RenderTargetId::Scratch(_) => self.scratch,
        };
        let result = self.paint(pixels, &mut pixels);
        match target {
            RenderTargetId::Main => self.main = pixels,
            RenderTargetId::Scratch(_) => self.scratch = pixels,
        }
        result
    }
}

fn ops() -> Vec<ExecOp> {
    (0..3)
        .map(|id| ExecOp::DrawBatch {
            draws: Rc::new(vec![id as usize]),
            batch_id: id,
            owners: Rc::new(Vec::new()),
            layer_stack: 0..0,
        })
        .collect()
}

#[test]
fn plan_order_skips_dead_batches_and_preserves_painter_order() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.persistent_root = Some(crate::RetainedNodeId::for_owner(1));
    canvas.painter.batch_counts = vec![1, 0, 1];
    let mut adapter = MemoryAdapter::new(false);
    execute_in_place_root_batches(&mut adapter, &canvas, &ops(), &[2, 1, 0]).unwrap();
    assert_eq!(adapter.main, [131, 231, 331, 431]);
    assert_eq!(adapter.coarse_batches, [2, 0]);
    assert_eq!(adapter.stats.draw_batches, 2);
    assert_eq!(adapter.stats.root_draw_batches, 2);
}

#[test]
fn partial_roots_preserve_undamaged_pixels_for_odd_and_even_batch_counts() {
    let canvas = Canvas::new(17, 19, 1.0);
    for (indices, expected) in [
        (vec![2], [13, 2, 33, 4]),
        (vec![2, 0], [131, 2, 331, 4]),
        (vec![2, 0, 2], [1313, 2, 3313, 4]),
    ] {
        let mut adapter = MemoryAdapter::new(true);
        execute_in_place_root_batches(&mut adapter, &canvas, &ops(), &indices).unwrap();
        assert_eq!(adapter.main, expected);
    }
}

#[test]
fn empty_and_dead_batches_do_no_work() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.persistent_root = Some(crate::RetainedNodeId::for_owner(1));
    canvas.painter.batch_counts = vec![0; 3];
    for indices in [&[][..], &[0, 1, 2][..]] {
        let mut adapter = MemoryAdapter::new(true);
        execute_in_place_root_batches(&mut adapter, &canvas, &ops(), indices).unwrap();
        assert_eq!(adapter.main, [1, 2, 3, 4]);
        assert!(adapter.coarse_batches.is_empty());
        assert_eq!(adapter.stats.draw_batches, 0);
    }
}

#[test]
fn a_failed_coarse_or_fine_stops_successors() {
    let canvas = Canvas::new(17, 19, 1.0);
    for failure in [Failure::Coarse(0), Failure::Fine(0)] {
        let mut adapter = MemoryAdapter::new(false);
        adapter.fail = Some(failure);
        assert_eq!(
            execute_in_place_root_batches(&mut adapter, &canvas, &ops(), &[2, 0, 1]),
            Err(failure)
        );
        assert_eq!(adapter.coarse_batches, [2, 0]);
    }
}

#[test]
fn scratch_draws_keep_root_statistics_separate() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut adapter = MemoryAdapter::new(false);
    execute_draw_batch(
        &mut adapter,
        &canvas,
        DrawBatch {
            draws: &[0],
            id: 0,
            layers: 0..0,
        },
        RenderTargetId::Scratch(0),
    )
    .unwrap();
    assert_eq!(adapter.scratch, [71, 81, 91, 101]);
    assert_eq!(adapter.main, [1, 2, 3, 4]);
    assert_eq!(adapter.stats.draw_batches, 1);
    assert_eq!(adapter.stats.root_draw_batches, 0);
}

#[test]
fn live_membership_uses_retained_counts_or_immediate_draws() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    assert!(!draw_batch_is_live(&canvas, &[], 0));
    assert!(draw_batch_is_live(&canvas, &[3], 0));
    canvas.persistent_root = Some(crate::RetainedNodeId::for_owner(1));
    canvas.painter.batch_counts = vec![2, 0];
    assert!(draw_batch_is_live(&canvas, &[], 0));
    assert!(!draw_batch_is_live(&canvas, &[3], 1));
    assert!(!draw_batch_is_live(&canvas, &[3], 2));
}
