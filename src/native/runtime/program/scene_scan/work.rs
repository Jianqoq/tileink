//! Reuse only accepted retained geometry; sparse work closes over whole paths.
use super::{PreparedScan, Result};
use crate::{
    Canvas,
    canvas::MaterializationStamp,
    shared::{dense_set::DenseIndexSet, gpu_plan::GpuCumsumPlan},
};
use std::{cell::Cell, rc::Rc};

pub(super) struct History {
    stamp: Option<MaterializationStamp>,
    accepted: Rc<Cell<bool>>,
}

impl History {
    pub(super) fn new(canvas: &Canvas, accepted: Rc<Cell<bool>>) -> Self {
        Self {
            stamp: canvas.materialization_stamp(),
            accepted,
        }
    }

    pub(super) fn contiguous(&self, canvas: &Canvas) -> bool {
        self.accepted.get() && canvas.journal_follows(self.stamp)
    }

    fn unchanged(&self, canvas: &Canvas) -> bool {
        self.stamp.is_some() && self.stamp == canvas.materialization_stamp()
    }
}

pub(super) struct ScanWork {
    pub(super) config: [u32; 11],
    pub(super) indices: Vec<u32>,
    pub(super) cumsum: Option<GpuCumsumPlan>,
    pub(super) reuse: bool,
}

impl ScanWork {
    pub(super) fn new(
        prepared: &PreparedScan<'_>,
        history: Option<&History>,
        preserve: bool,
        selected: &mut DenseIndexSet,
    ) -> Result<Self> {
        let lengths = &prepared.lengths;
        let mut full = Self {
            config: [
                0,
                u32::try_from(lengths.backdrop_len)?,
                u32::try_from(lengths.path_count)?,
                u32::try_from(lengths.scan_chunk_count)?,
                u32::try_from(lengths.line_count)?,
                u32::try_from(lengths.segment_capacity)?,
                0,
                0,
                0,
                0,
                0,
            ],
            indices: Vec::new(),
            cumsum: None,
            reuse: false,
        };
        full.config[0] = full.config[1].max(full.config[2]).max(full.config[3]);
        if !preserve || history.is_none_or(|h| !h.contiguous(prepared.canvas)) {
            return Ok(full);
        }
        if history.is_some_and(|h| h.unchanged(prepared.canvas)) {
            full.reuse = true;
            return Ok(full);
        }
        let canvas = prepared.canvas;
        let Some(changes) = &canvas.buffer_changes else {
            return Ok(full);
        };
        selected.begin(lengths.path_count);
        // A changed line requires all lines of its owner: tile counts, segment
        // prefixes and backdrop rows are shared by every edge in that path.
        for range in &changes.paths {
            for path in range.start.min(lengths.path_count)..range.end.min(lengths.path_count) {
                if canvas.path_records[path].is_live_at(path) {
                    selected.insert(path);
                }
            }
        }
        for range in &changes.lines {
            for line in &canvas.lines
                [range.start.min(lengths.line_count)..range.end.min(lengths.line_count)]
            {
                let path = line.path_id as usize;
                if canvas
                    .path_records
                    .get(path)
                    .is_some_and(|p| p.is_live_at(path))
                {
                    selected.insert(path);
                }
            }
        }
        if selected.is_empty() {
            full.reuse = true;
            return Ok(full);
        }
        if selected.len() == lengths.path_count {
            return Ok(full);
        }
        let paths: Vec<_> = (0..selected.len())
            .map(|i| selected.get(i) as u32)
            .collect();
        let mut counts = [0usize; 3];
        for &id in &paths {
            let path = &canvas.path_records[id as usize];
            let range = prepared.plans.scan_ranges()[id as usize];
            counts[0] += path.line_count as usize;
            counts[1] += (range.end - range.start) as usize;
            counts[2] += path.data_len as usize;
        }
        // Gathering/uploading IDs and indirect dispatch add work. Require at least
        // half the direct work to be avoided; vacant arena slots must not make
        // nearly full mutations pay for sparse preparation.
        let sparse_cost = counts[0] + counts[1] * 256 + counts[2] + paths.len();
        let full_cost = lengths.line_count
            + lengths.scan_chunk_count * 256
            + lengths.backdrop_len
            + lengths.path_count;
        if sparse_cost >= full_cost.div_ceil(2) {
            return Ok(full);
        }
        let mut indices = Vec::with_capacity(counts.iter().sum::<usize>() + paths.len());
        for &id in &paths {
            let path = &canvas.path_records[id as usize];
            indices.extend(path.line_start..path.line_start + path.line_count);
        }
        let path_base = u32::try_from(indices.len())?;
        indices.extend_from_slice(&paths);
        let chunk_base = u32::try_from(indices.len())?;
        for &id in &paths {
            let range = prepared.plans.scan_ranges()[id as usize];
            indices.extend(range.start..range.end);
        }
        let backdrop_base = u32::try_from(indices.len())?;
        for &id in &paths {
            let path = &canvas.path_records[id as usize];
            indices.extend(path.data_offset..path.data_offset + path.data_len);
        }
        let line_count = u32::try_from(counts[0])?;
        let chunk_count = u32::try_from(counts[1])?;
        let backdrop_count = u32::try_from(counts[2])?;
        let path_count = u32::try_from(paths.len())?;
        Ok(Self {
            config: [
                backdrop_count.max(path_count).max(chunk_count),
                backdrop_count,
                path_count,
                chunk_count,
                line_count,
                full.config[5],
                1,
                0,
                path_base,
                chunk_base,
                backdrop_base,
            ],
            cumsum: Some(prepared.plans.cumsum_for_paths(&paths)),
            indices,
            reuse: false,
        })
    }
}

#[cfg(test)]
#[path = "work/tests.rs"]
mod tests;
