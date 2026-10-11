//! Stable scan and cumulative-sum allocations for retained path slots.
#[cfg(test)]
use super::GpuBufferLengths;
use super::{CUMSUM_CHUNK_SIZE, GpuPathPlanCounts, SCAN_CHUNK_SIZE};
use crate::{
    canvas::Canvas,
    shared::{
        path::PathRecord,
        scene_arena::{ArenaAllocation, SceneArena},
    },
};
use bytemuck::{Pod, Zeroable};
use std::ops::Range;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct GpuScanChunk {
    pub path_id: u32,
    pub backdrop_offset: u32,
    pub segment_start: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct GpuScanChunkRange {
    pub start: u32,
    pub end: u32,
}

#[cfg(test)]
fn build_scan_chunks(canvas: &Canvas) -> (Vec<GpuScanChunk>, Vec<GpuScanChunkRange>) {
    let lengths = GpuBufferLengths::from_scene(canvas);
    let mut chunks = Vec::with_capacity(lengths.scan_chunk_count);
    let mut ranges = Vec::with_capacity(canvas.path_records.len());
    build_scan_chunks_into(canvas, lengths.scan_chunk_count, &mut chunks, &mut ranges);
    (chunks, ranges)
}

#[cfg(test)]
fn build_scan_chunks_into(
    canvas: &Canvas,
    scan_chunk_count: usize,
    chunks: &mut Vec<GpuScanChunk>,
    ranges: &mut Vec<GpuScanChunkRange>,
) {
    chunks.clear();
    chunks.reserve(scan_chunk_count);
    ranges.clear();
    ranges.resize(canvas.path_records.len(), GpuScanChunkRange::default());

    for record in &canvas.path_records {
        let range_start = chunks.len() as u32;
        let mut local = 0;
        while local < record.data_len {
            let len = (record.data_len - local).min(SCAN_CHUNK_SIZE);
            chunks.push(GpuScanChunk {
                path_id: record.path_id,
                backdrop_offset: record.data_offset + local,
                segment_start: record.segment_start,
                len,
            });
            local += len;
        }
        if let Some(range) = ranges.get_mut(record.path_id as usize) {
            *range = GpuScanChunkRange {
                start: range_start,
                end: chunks.len() as u32,
            };
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuCumsumPlan {
    pub chunk_backdrop_offsets: Vec<u32>,
    pub chunk_lens: Vec<u32>,
    pub row_chunk_starts: Vec<u32>,
    pub row_chunk_ends: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GpuCumsumChunk {
    backdrop_offset: u32,
    len: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GpuCumsumRow {
    chunk_start: u32,
    chunk_end: u32,
}

#[derive(Clone, Copy, Debug)]
struct PathPlanAllocation {
    scan: ArenaAllocation,
    cumsum_chunks: ArenaAllocation,
    cumsum_rows: ArenaAllocation,
}

#[derive(Debug, Default)]
pub(crate) struct GpuPathPlanDirty {
    pub(crate) scan_chunks: Vec<Range<usize>>,
    pub(crate) scan_ranges: Vec<Range<usize>>,
    pub(crate) cumsum_chunks: Vec<Range<usize>>,
    pub(crate) cumsum_rows: Vec<Range<usize>>,
}

/// Stable scan/cumsum allocations keyed by physical path slot.
///
/// Retained path records already use stable arena offsets. Keeping their derived GPU plans in
/// matching variable-sized allocations avoids rebuilding and diffing every path when one chunk
/// changes shape. Vacant arena slots compile to zero-length work and are therefore safe to leave
/// in the dispatch address space until a normal fragmentation-triggered compaction.
pub(crate) struct PersistentPathPlans {
    scan_chunks: SceneArena<GpuScanChunk>,
    scan_ranges: Vec<GpuScanChunkRange>,
    cumsum_chunks: SceneArena<GpuCumsumChunk>,
    cumsum_rows: SceneArena<GpuCumsumRow>,
    cumsum_plan: GpuCumsumPlan,
    allocations: Vec<Option<PathPlanAllocation>>,
    row_chunk_counts: Vec<Option<Vec<u32>>>,
    initialized: bool,
    dirty_scan_chunks: Vec<Range<usize>>,
    dirty_scan_ranges: Vec<Range<usize>>,
    dirty_cumsum_chunks: Vec<Range<usize>>,
    dirty_cumsum_rows: Vec<Range<usize>>,
    arena_dirty_ranges: Vec<Range<usize>>,
}

impl Default for PersistentPathPlans {
    fn default() -> Self {
        Self {
            scan_chunks: SceneArena::new(GpuScanChunk::default()),
            scan_ranges: Vec::new(),
            cumsum_chunks: SceneArena::new(GpuCumsumChunk::default()),
            cumsum_rows: SceneArena::new(GpuCumsumRow::default()),
            cumsum_plan: GpuCumsumPlan::default(),
            allocations: Vec::new(),
            row_chunk_counts: Vec::new(),
            initialized: false,
            dirty_scan_chunks: Vec::new(),
            dirty_scan_ranges: Vec::new(),
            dirty_cumsum_chunks: Vec::new(),
            dirty_cumsum_rows: Vec::new(),
            arena_dirty_ranges: Vec::new(),
        }
    }
}

impl PersistentPathPlans {
    pub(crate) fn update(
        &mut self,
        canvas: &Canvas,
        changed_ranges: Option<&[Range<usize>]>,
    ) -> GpuPathPlanCounts {
        let path_len = canvas.path_records.len();
        let old_len = self.allocations.len();
        let full = !self.initialized || changed_ranges.is_none();

        let scan_compactions = self.scan_chunks.compactions();
        let cumsum_compactions = self.cumsum_chunks.compactions();
        if full {
            *self = Self::default();
            self.allocations.resize(path_len, None);
            self.row_chunk_counts.resize(path_len, None);
            self.scan_ranges
                .resize(path_len, GpuScanChunkRange::default());
            if path_len != 0 {
                self.dirty_scan_ranges.push(0..path_len);
            }
        } else {
            let removed = self
                .allocations
                .drain(path_len.min(old_len)..)
                .flatten()
                .collect::<Vec<_>>();
            for allocation in removed {
                self.remove_allocation(allocation);
            }
            self.allocations.resize(path_len, None);
            self.row_chunk_counts.truncate(path_len);
            self.row_chunk_counts.resize(path_len, None);
            self.scan_ranges
                .resize(path_len, GpuScanChunkRange::default());
            // A regrown vacant slot is zero on the CPU, but a retained GPU
            // allocation can still contain its old range. Publish the entire
            // exposed suffix even when update_path sees no value change.
            if path_len > old_len {
                merge_range(&mut self.dirty_scan_ranges, old_len..path_len);
            }
        }

        if full {
            for (path_id, record) in canvas.path_records.iter().enumerate() {
                self.update_path(path_id, record);
            }
        } else {
            let mut changed = Vec::new();
            for range in changed_ranges.unwrap() {
                let start = range.start.min(path_len);
                let end = range.end.min(path_len);
                changed.extend(start..end);
            }
            if path_len > old_len {
                changed.extend(old_len..path_len);
            }
            changed.sort_unstable();
            changed.dedup();
            for path_id in changed {
                self.update_path(path_id, &canvas.path_records[path_id]);
            }
        }

        if self.scan_chunks.compactions() != scan_compactions {
            self.refresh_all_scan_ranges();
        }
        if self.cumsum_chunks.compactions() != cumsum_compactions {
            self.refresh_all_cumsum_rows();
        }
        self.initialized = true;
        self.sync_dirty_outputs();
        self.counts()
    }

    fn update_path(&mut self, path_id: usize, record: &PathRecord) {
        if !record.is_live_at(path_id) {
            if let Some(allocation) = self.allocations[path_id].take() {
                self.remove_allocation(allocation);
            }
            self.row_chunk_counts[path_id] = None;
            self.set_scan_range(path_id, GpuScanChunkRange::default());
            return;
        }

        let scan = scan_chunks_for_record(record);
        let (cumsum_chunks, row_chunk_counts) = cumsum_for_record(record);
        let allocation = if let Some(allocation) = self.allocations[path_id] {
            self.scan_chunks.replace(allocation.scan, &scan);
            self.cumsum_chunks
                .replace(allocation.cumsum_chunks, &cumsum_chunks);
            allocation
        } else {
            PathPlanAllocation {
                scan: self.scan_chunks.insert(&scan),
                cumsum_chunks: self.cumsum_chunks.insert(&cumsum_chunks),
                cumsum_rows: self.cumsum_rows.insert(&[]),
            }
        };
        self.allocations[path_id] = Some(allocation);
        self.row_chunk_counts[path_id] = Some(row_chunk_counts.clone());
        self.refresh_scan_range(path_id);
        self.refresh_cumsum_rows(path_id, &row_chunk_counts);
    }

    fn remove_allocation(&mut self, allocation: PathPlanAllocation) {
        self.scan_chunks.remove(allocation.scan);
        self.cumsum_chunks.remove(allocation.cumsum_chunks);
        self.cumsum_rows.remove(allocation.cumsum_rows);
    }

    fn refresh_scan_range(&mut self, path_id: usize) {
        let Some(allocation) = self.allocations[path_id] else {
            self.set_scan_range(path_id, GpuScanChunkRange::default());
            return;
        };
        let range = self.scan_chunks.range(allocation.scan);
        self.set_scan_range(
            path_id,
            GpuScanChunkRange {
                start: range.start as u32,
                end: range.end as u32,
            },
        );
    }

    fn set_scan_range(&mut self, path_id: usize, range: GpuScanChunkRange) {
        if self.scan_ranges[path_id] != range {
            self.scan_ranges[path_id] = range;
            merge_range(&mut self.dirty_scan_ranges, path_id..path_id + 1);
        }
    }

    fn refresh_all_scan_ranges(&mut self) {
        for path_id in 0..self.allocations.len() {
            self.refresh_scan_range(path_id);
        }
    }

    fn refresh_cumsum_rows(&mut self, path_id: usize, row_chunk_counts: &[u32]) {
        let allocation = self.allocations[path_id].unwrap();
        let mut chunk = self.cumsum_chunks.range(allocation.cumsum_chunks).start as u32;
        let rows = row_chunk_counts
            .iter()
            .map(|&count| {
                let row = GpuCumsumRow {
                    chunk_start: chunk,
                    chunk_end: chunk + count,
                };
                chunk += count;
                row
            })
            .collect::<Vec<_>>();
        self.cumsum_rows.replace(allocation.cumsum_rows, &rows);
    }

    fn refresh_all_cumsum_rows(&mut self) {
        for path_id in 0..self.allocations.len() {
            let Some(allocation) = self.allocations[path_id] else {
                continue;
            };
            let record_chunks = self.cumsum_chunks.range(allocation.cumsum_chunks);
            let counts = self.row_chunk_counts[path_id]
                .as_deref()
                .expect("live path plan has cumsum row metadata");
            let mut chunk = record_chunks.start as u32;
            let rows = counts
                .iter()
                .map(|&count| {
                    let row = GpuCumsumRow {
                        chunk_start: chunk,
                        chunk_end: chunk + count,
                    };
                    chunk += count;
                    row
                })
                .collect::<Vec<_>>();
            self.cumsum_rows.replace(allocation.cumsum_rows, &rows);
        }
    }

    fn sync_dirty_outputs(&mut self) {
        self.scan_chunks
            .take_dirty_ranges_into(&mut self.arena_dirty_ranges);
        merge_ranges(
            &mut self.dirty_scan_chunks,
            self.arena_dirty_ranges.drain(..),
        );
        self.cumsum_chunks
            .take_dirty_ranges_into(&mut self.arena_dirty_ranges);
        self.cumsum_plan
            .chunk_backdrop_offsets
            .resize(self.cumsum_chunks.values().len(), 0);
        self.cumsum_plan
            .chunk_lens
            .resize(self.cumsum_chunks.values().len(), 0);
        for range in &self.arena_dirty_ranges {
            for (index, chunk) in self.cumsum_chunks.values()[range.clone()]
                .iter()
                .enumerate()
            {
                let index = range.start + index;
                self.cumsum_plan.chunk_backdrop_offsets[index] = chunk.backdrop_offset;
                self.cumsum_plan.chunk_lens[index] = chunk.len;
            }
        }
        merge_ranges(
            &mut self.dirty_cumsum_chunks,
            self.arena_dirty_ranges.drain(..),
        );

        self.cumsum_rows
            .take_dirty_ranges_into(&mut self.arena_dirty_ranges);
        self.cumsum_plan
            .row_chunk_starts
            .resize(self.cumsum_rows.values().len(), 0);
        self.cumsum_plan
            .row_chunk_ends
            .resize(self.cumsum_rows.values().len(), 0);
        for range in &self.arena_dirty_ranges {
            for (index, row) in self.cumsum_rows.values()[range.clone()].iter().enumerate() {
                let index = range.start + index;
                self.cumsum_plan.row_chunk_starts[index] = row.chunk_start;
                self.cumsum_plan.row_chunk_ends[index] = row.chunk_end;
            }
        }
        merge_ranges(
            &mut self.dirty_cumsum_rows,
            self.arena_dirty_ranges.drain(..),
        );
    }

    pub(crate) fn counts(&self) -> GpuPathPlanCounts {
        GpuPathPlanCounts {
            scan_chunks: self.scan_chunks.values().len(),
            cumsum_chunks: self.cumsum_chunks.values().len(),
            cumsum_rows: self.cumsum_rows.values().len(),
        }
    }

    pub(crate) fn scan_chunks(&self) -> &[GpuScanChunk] {
        self.scan_chunks.values()
    }

    pub(crate) fn scan_ranges(&self) -> &[GpuScanChunkRange] {
        &self.scan_ranges
    }

    pub(crate) fn cumsum_plan(&self) -> &GpuCumsumPlan {
        &self.cumsum_plan
    }

    pub(crate) fn take_dirty(&mut self) -> GpuPathPlanDirty {
        GpuPathPlanDirty {
            scan_chunks: std::mem::take(&mut self.dirty_scan_chunks),
            scan_ranges: std::mem::take(&mut self.dirty_scan_ranges),
            cumsum_chunks: std::mem::take(&mut self.dirty_cumsum_chunks),
            cumsum_rows: std::mem::take(&mut self.dirty_cumsum_rows),
        }
    }

    /// Returns upload-consumed range storage so each retained frame keeps its peak capacity.
    pub(crate) fn recycle_dirty(&mut self, mut dirty: GpuPathPlanDirty) {
        debug_assert!(self.dirty_scan_chunks.is_empty());
        debug_assert!(self.dirty_scan_ranges.is_empty());
        debug_assert!(self.dirty_cumsum_chunks.is_empty());
        debug_assert!(self.dirty_cumsum_rows.is_empty());
        dirty.scan_chunks.clear();
        dirty.scan_ranges.clear();
        dirty.cumsum_chunks.clear();
        dirty.cumsum_rows.clear();
        self.dirty_scan_chunks = dirty.scan_chunks;
        self.dirty_scan_ranges = dirty.scan_ranges;
        self.dirty_cumsum_chunks = dirty.cumsum_chunks;
        self.dirty_cumsum_rows = dirty.cumsum_rows;
    }
}

fn scan_chunks_for_record(record: &PathRecord) -> Vec<GpuScanChunk> {
    let mut chunks = Vec::with_capacity(record.data_len.div_ceil(SCAN_CHUNK_SIZE) as usize);
    let mut local = 0;
    while local < record.data_len {
        let len = (record.data_len - local).min(SCAN_CHUNK_SIZE);
        chunks.push(GpuScanChunk {
            path_id: record.path_id,
            backdrop_offset: record.data_offset + local,
            segment_start: record.segment_start,
            len,
        });
        local += len;
    }
    chunks
}

fn cumsum_for_record(record: &PathRecord) -> (Vec<GpuCumsumChunk>, Vec<u32>) {
    let stride = record.tile_x1.saturating_sub(record.tile_x0);
    let height = record.tile_y1.saturating_sub(record.tile_y0);
    let mut chunks = Vec::new();
    let mut rows = Vec::new();
    if stride == 0 || height == 0 {
        return (chunks, rows);
    }
    chunks.reserve((height * stride.div_ceil(CUMSUM_CHUNK_SIZE)) as usize);
    rows.reserve(height as usize);
    for row in 0..height {
        let before = chunks.len();
        let row_offset = record.data_offset + row * stride;
        let mut local_x = 0;
        while local_x < stride {
            let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
            chunks.push(GpuCumsumChunk {
                backdrop_offset: row_offset + local_x,
                len,
            });
            local_x += len;
        }
        rows.push((chunks.len() - before) as u32);
    }
    (chunks, rows)
}

fn merge_ranges(target: &mut Vec<Range<usize>>, ranges: impl IntoIterator<Item = Range<usize>>) {
    for range in ranges {
        merge_range(target, range);
    }
}

fn merge_range(target: &mut Vec<Range<usize>>, mut range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    let mut index = 0;
    while index < target.len() {
        if target[index].end < range.start || range.end < target[index].start {
            index += 1;
            continue;
        }
        let current = target.swap_remove(index);
        range.start = range.start.min(current.start);
        range.end = range.end.max(current.end);
    }
    target.push(range);
    target.sort_unstable_by_key(|range| range.start);
}

#[cfg(test)]
fn build_cumsum_plan(canvas: &Canvas) -> GpuCumsumPlan {
    let lengths = GpuBufferLengths::from_scene(canvas);
    let mut plan = GpuCumsumPlan::default();
    build_cumsum_plan_into(canvas, lengths, &mut plan);
    plan
}

#[cfg(test)]
fn build_cumsum_plan_into(canvas: &Canvas, lengths: GpuBufferLengths, plan: &mut GpuCumsumPlan) {
    plan.chunk_backdrop_offsets.clear();
    plan.chunk_lens.clear();
    plan.row_chunk_starts.clear();
    plan.row_chunk_ends.clear();
    plan.chunk_backdrop_offsets
        .reserve(lengths.cumsum_chunk_count);
    plan.chunk_lens.reserve(lengths.cumsum_chunk_count);
    plan.row_chunk_starts.reserve(lengths.cumsum_row_count);
    plan.row_chunk_ends.reserve(lengths.cumsum_row_count);

    for record in &canvas.path_records {
        let stride = record.tile_x1.saturating_sub(record.tile_x0);
        let height = record.tile_y1.saturating_sub(record.tile_y0);
        if stride == 0 || height == 0 {
            continue;
        }

        for row in 0..height {
            let row_start = plan.chunk_backdrop_offsets.len() as u32;
            let row_offset = record.data_offset + row * stride;
            let mut local_x = 0;
            while local_x < stride {
                let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
                plan.chunk_backdrop_offsets.push(row_offset + local_x);
                plan.chunk_lens.push(len);
                local_x += len;
            }
            plan.row_chunk_starts.push(row_start);
            plan.row_chunk_ends
                .push(plan.chunk_backdrop_offsets.len() as u32);
        }
    }
}

#[cfg(test)]
#[path = "path_plan_tests.rs"]
mod tests;
