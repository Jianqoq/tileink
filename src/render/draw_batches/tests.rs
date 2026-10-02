use super::*;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Targets,
    RootSubmission,
    Coarse(u32),
    Fine(u32),
}

struct MemoryAdapter {
    main: [u32; 4],
    scratch: [u32; 4],
    ping: [[u32; 4]; 2],
    partial: bool,
    stats: IncrementalRenderStats,
    root_batches: usize,
    coarse_batches: Vec<u32>,
    last_batch: u32,
    final_copies: usize,
    fail: Option<Failure>,
}

impl MemoryAdapter {
    fn new(partial: bool) -> Self {
        Self {
            main: [1, 2, 3, 4],
            scratch: [7, 8, 9, 10],
            ping: [[999; 4]; 2],
            partial,
            stats: IncrementalRenderStats::default(),
            root_batches: 0,
            coarse_batches: Vec::new(),
            last_batch: 0,
            final_copies: 0,
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

    fn begin_root_batch(&mut self) -> Result<(), Failure> {
        self.root_batches += 1;
        if self.fail == Some(Failure::RootSubmission) {
            Err(Failure::RootSubmission)
        } else {
            Ok(())
        }
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

impl RootBatchAdapter for MemoryAdapter {
    fn prepare_portable_targets(&mut self) -> Result<(), Failure> {
        if self.fail == Some(Failure::Targets) {
            Err(Failure::Targets)
        } else {
            Ok(())
        }
    }

    fn copy_root_to(&mut self, side: PingPongSide) -> Result<(), Failure> {
        self.ping[side as usize] = self.main;
        Ok(())
    }

    fn copy_to_root(&mut self, side: PingPongSide) -> Result<(), Failure> {
        self.main = self.ping[side as usize];
        self.final_copies += 1;
        Ok(())
    }

    fn fine_portable(&mut self, source: PingPongSide) -> Result<(), Failure> {
        let mut pixels = self.ping[source.other() as usize];
        let result = self.paint(self.ping[source as usize], &mut pixels);
        self.ping[source.other() as usize] = pixels;
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
fn plan_order_and_dead_batches_have_identical_direct_and_portable_pixels() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.stable_batch_counts = Some(vec![1, 0, 1]);
    for mode in [
        RootBatchMode::Direct,
        RootBatchMode::Portable { partial: false },
    ] {
        let mut adapter = MemoryAdapter::new(false);
        execute_direct_root_batches(&mut adapter, &canvas, &ops(), &[2, 1, 0], mode).unwrap();
        assert_eq!(adapter.main, [131, 231, 331, 431]);
        assert_eq!(adapter.coarse_batches, [2, 0]);
        assert_eq!(adapter.root_batches, 2);
        assert_eq!(adapter.stats.draw_batches, 2);
        assert_eq!(adapter.stats.root_draw_batches, 2);
    }
}

#[test]
fn partial_ping_pong_preserves_undamaged_pixels_for_odd_and_even_batch_counts() {
    let canvas = Canvas::new(17, 19, 1.0);
    for (indices, expected) in [
        (vec![2], [13, 2, 33, 4]),
        (vec![2, 0], [131, 2, 331, 4]),
        (vec![2, 0, 2], [1313, 2, 3313, 4]),
    ] {
        let mut adapter = MemoryAdapter::new(true);
        execute_direct_root_batches(
            &mut adapter,
            &canvas,
            &ops(),
            &indices,
            RootBatchMode::Portable { partial: true },
        )
        .unwrap();
        assert_eq!(adapter.main, expected);
        assert_eq!(adapter.stats.portable_texture_copies, 3);
        assert_eq!(adapter.final_copies, 1);
    }
}

#[test]
fn no_live_batches_preserve_history_without_a_final_copy_or_submission_budget() {
    let mut canvas = Canvas::new(17, 19, 1.0);
    canvas.stable_batch_counts = Some(vec![0; 3]);
    let mut adapter = MemoryAdapter::new(true);
    execute_direct_root_batches(
        &mut adapter,
        &canvas,
        &ops(),
        &[0, 1, 2],
        RootBatchMode::Portable { partial: true },
    )
    .unwrap();
    assert_eq!(adapter.main, [1, 2, 3, 4]);
    assert_eq!(adapter.root_batches, 0);
    assert_eq!(adapter.stats.draw_batches, 0);
    assert_eq!(adapter.stats.portable_texture_copies, 2);
    assert_eq!(adapter.final_copies, 0);
}

#[test]
fn an_empty_direct_list_needs_no_portable_allocations() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut adapter = MemoryAdapter::new(false);
    adapter.fail = Some(Failure::Targets);
    execute_direct_root_batches(
        &mut adapter,
        &canvas,
        &ops(),
        &[],
        RootBatchMode::Portable { partial: false },
    )
    .unwrap();
    assert_eq!(adapter.stats.portable_texture_copies, 0);
}

#[test]
fn a_failed_coarse_or_fine_stops_successors_and_never_publishes_portable_work() {
    let canvas = Canvas::new(17, 19, 1.0);
    for failure in [Failure::Coarse(0), Failure::Fine(0)] {
        let mut adapter = MemoryAdapter::new(false);
        adapter.fail = Some(failure);
        assert_eq!(
            execute_direct_root_batches(
                &mut adapter,
                &canvas,
                &ops(),
                &[2, 0, 1],
                RootBatchMode::Portable { partial: false }
            ),
            Err(failure)
        );
        assert_eq!(adapter.main, [1, 2, 3, 4]);
        assert_eq!(adapter.coarse_batches, [2, 0]);
        assert_eq!(adapter.final_copies, 0);
        assert_eq!(adapter.stats.portable_texture_copies, 1);
    }
}

#[test]
fn scratch_draws_do_not_consume_the_main_target_submission_budget() {
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
    assert_eq!(adapter.root_batches, 0);
}

#[test]
fn target_preparation_failure_records_no_copies_or_draws() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut adapter = MemoryAdapter::new(false);
    adapter.fail = Some(Failure::Targets);
    assert_eq!(
        execute_direct_root_batches(
            &mut adapter,
            &canvas,
            &ops(),
            &[0],
            RootBatchMode::Portable { partial: false }
        ),
        Err(Failure::Targets)
    );
    assert_eq!(adapter.main, [1, 2, 3, 4]);
    assert_eq!(adapter.stats.portable_texture_copies, 0);
    assert_eq!(adapter.stats.draw_batches, 0);
}

#[test]
fn an_early_submission_failure_reaches_the_owner_without_encoding_a_new_batch() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut adapter = MemoryAdapter::new(false);
    adapter.fail = Some(Failure::RootSubmission);
    assert_eq!(
        execute_direct_root_batches(
            &mut adapter,
            &canvas,
            &ops(),
            &[0, 1],
            RootBatchMode::Direct
        ),
        Err(Failure::RootSubmission)
    );
    assert_eq!(adapter.main, [1, 2, 3, 4]);
    assert!(adapter.coarse_batches.is_empty());
    assert_eq!(adapter.stats.draw_batches, 0);
}
