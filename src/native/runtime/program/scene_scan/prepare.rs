use crate::{Canvas, shared::gpu_plan::PersistentPathPlans};

/// A single preparation owns the association between source records and plans.
/// Borrowing both inputs prevents mutation until all dispatches are recorded.
pub struct PreparedScan<'a> {
    pub(super) canvas: &'a Canvas,
    pub(super) plans: &'a PersistentPathPlans,
    pub(super) lengths: ScanLengths,
}

pub(super) struct ScanLengths {
    pub line_count: usize,
    pub path_count: usize,
    pub scan_chunk_count: usize,
    pub backdrop_len: usize,
    pub segment_capacity: usize,
}

impl<'a> PreparedScan<'a> {
    pub fn new(canvas: &'a Canvas, plans: &'a mut PersistentPathPlans) -> Self {
        // Reconcile from this Canvas, not a caller-supplied dirty hint or stale
        // length tuple. This fixes the unsafe association at its source.
        plans.update(canvas, None);
        let lengths = ScanLengths {
            line_count: canvas.lines.len(),
            path_count: canvas.path_records.len(),
            scan_chunk_count: plans.scan_chunks().len(),
            backdrop_len: canvas.backdrop_pool_capacity as usize,
            segment_capacity: canvas.tile_cnt as usize,
        };
        Self {
            canvas,
            plans,
            lengths,
        }
    }
}
