use std::sync::atomic::{AtomicU32, Ordering};

use rayon::prelude::*;

use crate::{
    TILE_SIZE,
    shared::{
        bd_record::BackdropRecord,
        bounds::TileBbox,
        line::Line,
        line_seg::LineSegment,
        path::{PATH_FLAG_KEEP_HORIZONTAL_TILE_EDGES, PathRecord},
        scan_line::{TILE_BOUNDARY_EPSILON, for_each_scanned_tile, plan_scan_line},
        tile_seg_range::TileSegmentRange,
    },
};

/// wgpu compute scan_assign: one workgroup per path.
pub struct ScanCpuPipeline {}

pub struct ScanCpuPrepared<'a> {
    lines: &'a [Line],
    path_records: &'a [PathRecord],
    backdrop_records: &'a [BackdropRecord],
    backdrops: &'a mut Vec<i32>, // [path_id][tile_y][tile_x]
    tile_segment_ranges: &'a mut Vec<TileSegmentRange>,
    segments: &'a mut Vec<LineSegment>,
    segments_bump: &'a mut Vec<AtomicU32>,
    segment_tile_counts: &'a mut Vec<u32>,
    segment_tile_cursors: &'a mut Vec<AtomicU32>,
    tiles_size: (u32, u32),
}

impl<'a> ScanCpuPrepared<'a> {
    pub fn run(&mut self) {
        let backdrops = self.backdrops.as_mut_ptr() as *mut Vec<i32> as usize;
        let ranges = self.tile_segment_ranges.as_mut_ptr() as usize;
        let segments = self.segments.as_mut_ptr() as usize;
        let segment_tile_counts = self.segment_tile_counts.as_mut_ptr() as *mut Vec<u32> as usize;
        (0..self.backdrop_records.len())
            .into_par_iter()
            .for_each(|record_ix| {
                let backdrop_record = &self.backdrop_records[record_ix];
                let path_id = backdrop_record.path_id as usize;
                let path_record = &self.path_records[path_id];
                let bbox = TileBbox {
                    x0: backdrop_record.tile_x0,
                    y0: backdrop_record.tile_y0,
                    x1: backdrop_record.tile_x1,
                    y1: backdrop_record.tile_y1,
                };
                let tile_count = backdrop_record.data_len as usize;
                let data_offset = backdrop_record.data_offset as usize;
                if tile_count == 0 {
                    debug_assert_eq!(bbox.tile_count(), 0);
                    return;
                }

                let backdrop_ptr = backdrops as *mut i32;
                let backdrops =
                    unsafe { std::slice::from_raw_parts_mut(backdrop_ptr, self.backdrops.len()) };
                let backdrop = &mut backdrops[data_offset..data_offset + tile_count];
                backdrop.fill(0);

                let ranges_ptr = ranges as *mut TileSegmentRange;
                let ranges = unsafe {
                    std::slice::from_raw_parts_mut(ranges_ptr.add(data_offset), tile_count)
                };
                ranges.fill(TileSegmentRange::default());

                let counts_ptr = segment_tile_counts as *mut u32;
                let counts = &mut unsafe {
                    std::slice::from_raw_parts_mut(counts_ptr, self.segment_tile_counts.len())
                }[data_offset..data_offset + tile_count];
                for count in counts.iter_mut() {
                    *count = 0;
                }

                let line_start = path_record.line_start as usize;
                let line_end = line_start + path_record.line_count as usize;
                let lines = &self.lines[line_start..line_end];
                let keep_horizontal_tile_edges =
                    path_record.flags & PATH_FLAG_KEEP_HORIZONTAL_TILE_EDGES != 0;

                for &line in lines {
                    if let Some(plan) = plan_scan_line(line, bbox, keep_horizontal_tile_edges) {
                        for y in plan.ymin..plan.ymax {
                            let base = ((y - bbox.y0 as i32) * bbox.tile_stride() as i32) as usize;
                            backdrop[base] += plan.delta;
                        }
                        if let Some(x_bump) = plan.top_clip_bump_x
                            && x_bump >= bbox.x0 as i32
                            && x_bump < bbox.x1 as i32
                        {
                            backdrop[(x_bump - bbox.x0 as i32) as usize] += plan.delta;
                        }

                        // Stroke outlines keep horizontal boundary edges as fine segments; those
                        // edges must not also emit coarse top-edge carry. Fill paths keep the
                        // historical top-edge carry because their boundary horizontals are owned
                        // by the coarse scan rule instead of fine segments.
                        let needs_top_edge_carry = !plan.keep_horizontal_tile_edges
                            || (plan.xy0[1] != plan.xy1[1]
                                && ((plan.xy0[1] * crate::TILE_SCALE).floor()
                                    < (plan.xy1[1] * crate::TILE_SCALE).floor()
                                    || bbox.y1 > bbox.y0 + 1));
                        let skip_initial_top_edge_carry = plan.keep_horizontal_tile_edges
                            && bbox.y0 == 0
                            && plan.xy1[0] < plan.xy0[0];
                        for_each_scanned_tile(&plan, bbox, self.tiles_size, |tile| {
                            if needs_top_edge_carry
                                && tile.top_edge
                                && !(tile.initial_top_edge && skip_initial_top_edge_carry)
                                && tile.x + 1 < bbox.x1 as i32
                            {
                                let x_bump = (tile.x + 1).max(bbox.x0 as i32);
                                let bump_ix =
                                    ((tile.y - bbox.y0 as i32) * bbox.tile_stride() as i32 + x_bump
                                        - bbox.x0 as i32)
                                        as usize;
                                backdrop[bump_ix] += plan.delta;
                            }

                            let local_ix = Self::local_tile_ix(
                                backdrop_record,
                                tile.global_ix,
                                self.tiles_size.0,
                            );
                            let count = counts[local_ix];
                            counts[local_ix] = count + 1;
                        });
                    }
                }

                let cursors = &self.segment_tile_cursors[data_offset..data_offset + tile_count];
                let mut next = backdrop_record.segment_start;
                for ((range, count), cursor) in ranges.iter_mut().zip(counts).zip(cursors) {
                    let count = *count;
                    range.start = next;
                    next += count;
                    range.end = next;
                    cursor.store(range.start, Ordering::Relaxed);
                }

                let segment_count = next - backdrop_record.segment_start;
                self.segments_bump[path_id].store(segment_count, Ordering::Relaxed);
                debug_assert!(
                    segment_count <= backdrop_record.segment_capacity,
                    "scan emitted more segments than reserved capacity"
                );

                if segment_count == 0 {
                    return;
                }

                let segments_ptr = segments as *mut LineSegment;
                let segments =
                    unsafe { std::slice::from_raw_parts_mut(segments_ptr, self.segments.len()) };

                for &line in lines {
                    if let Some(plan) = plan_scan_line(line, bbox, keep_horizontal_tile_edges) {
                        for_each_scanned_tile(&plan, bbox, self.tiles_size, |tile| {
                            let segment = clip_line_to_tile(
                                (plan.xy0, plan.xy1),
                                plan.is_down,
                                plan.keep_horizontal_tile_edges,
                                tile.x,
                                tile.y,
                            );
                            let local_ix = Self::local_tile_ix(
                                backdrop_record,
                                tile.global_ix,
                                self.tiles_size.0,
                            );
                            let dst = cursors[local_ix].load(Ordering::Relaxed);
                            cursors[local_ix].store(dst + 1, Ordering::Relaxed);
                            debug_assert!(
                                dst < backdrop_record.segment_start
                                    + backdrop_record.segment_capacity
                            );
                            segments[dst as usize] = segment;
                        });
                    }
                }

                debug_assert!(
                    ranges
                        .iter()
                        .zip(cursors.iter())
                        .all(|(range, cursor)| { range.end == cursor.load(Ordering::Relaxed) })
                );
            });
    }

    fn local_tile_ix(backdrop_record: &BackdropRecord, tile_id: u32, tiles_width: u32) -> usize {
        let tile_x = tile_id % tiles_width;
        let tile_y = tile_id / tiles_width;
        let local_x = tile_x - backdrop_record.tile_x0;
        let local_y = tile_y - backdrop_record.tile_y0;
        let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
        (local_y * stride + local_x) as usize
    }

    #[cfg(test)]
    fn fill_ranges_from_counts_parallel(
        ranges: &mut [TileSegmentRange],
        counts: &[AtomicU32],
        start: u32,
    ) {
        debug_assert_eq!(ranges.len(), counts.len());
        if ranges.is_empty() {
            return;
        }

        let worker_count = rayon::current_num_threads().max(1);
        let chunk_count = ranges.len().min(worker_count * 4).max(1);
        let chunk_len = ranges.len().div_ceil(chunk_count);
        let mut chunk_totals = vec![0u32; ranges.len().div_ceil(chunk_len)];

        ranges
            .par_chunks_mut(chunk_len)
            .zip(counts.par_chunks(chunk_len))
            .zip(chunk_totals.par_iter_mut())
            .for_each(|((range_chunk, count_chunk), chunk_total)| {
                let mut next = 0u32;
                for (range, count) in range_chunk.iter_mut().zip(count_chunk.iter()) {
                    let count = count.load(Ordering::Relaxed);
                    range.start = next;
                    next += count;
                    range.end = next;
                }
                *chunk_total = next;
            });

        let mut chunk_offsets = Vec::with_capacity(chunk_totals.len());
        let mut next = start;
        for total in chunk_totals {
            chunk_offsets.push(next);
            next += total;
        }

        ranges
            .par_chunks_mut(chunk_len)
            .zip(chunk_offsets.into_par_iter())
            .for_each(|(range_chunk, offset)| {
                for range in range_chunk {
                    range.start += offset;
                    range.end += offset;
                }
            });
    }
}

impl ScanCpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        lines: &'a [Line],
        path_records: &'a [PathRecord],
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a mut Vec<i32>,
        tile_segment_ranges: &'a mut Vec<TileSegmentRange>,
        segments: &'a mut Vec<LineSegment>,
        segments_bump: &'a mut Vec<AtomicU32>,
        segment_tile_counts: &'a mut Vec<u32>,
        segment_tile_cursors: &'a mut Vec<AtomicU32>,
        tiles_size: (u32, u32),
    ) -> ScanCpuPrepared<'a> {
        ScanCpuPrepared {
            lines,
            path_records,
            backdrop_records,
            tiles_size,
            backdrops,
            tile_segment_ranges,
            segments,
            segments_bump,
            segment_tile_counts,
            segment_tile_cursors,
        }
    }
}

fn clip_line_to_tile(
    line: ([f32; 2], [f32; 2]),
    is_down: bool,
    keep_horizontal_tile_edges: bool,
    tile_x: i32,
    tile_y: i32,
) -> LineSegment {
    let (line0, line1) = line;
    let tile_xy = [
        tile_x as f32 * TILE_SIZE as f32,
        tile_y as f32 * TILE_SIZE as f32,
    ];
    let tile_xy1 = [tile_xy[0] + TILE_SIZE as f32, tile_xy[1] + TILE_SIZE as f32];

    let (xy0, xy1) = clip_segment_to_tile(line0, line1, tile_xy, tile_xy1);

    let mut y_edge = 1e9f32;
    let mut p0 = (
        clamp_tile_coord(xy0[0] - tile_xy[0]),
        clamp_tile_coord(xy0[1] - tile_xy[1]),
    );
    let mut p1 = (
        clamp_tile_coord(xy1[0] - tile_xy[0]),
        clamp_tile_coord(xy1[1] - tile_xy[1]),
    );
    const EPSILON: f32 = 1e-6;

    if p0.0 == 0.0 {
        if p1.0 == 0.0 {
            p0.0 = EPSILON;
            if p0.1 == 0.0 {
                p1.0 = EPSILON;
                p1.1 = TILE_SIZE as f32;
            } else {
                p1.0 = 2.0 * EPSILON;
                p1.1 = p0.1;
            }
        } else if p0.1 == 0.0 {
            // Diagonal edges passing exactly through top-left are owned by the top edge.
            // Stroke outlines keep horizontal top edges on tile boundaries so their paired
            // bottom edges cannot fill every row below the stroke.
            if (keep_horizontal_tile_edges && p1.1 == 0.0)
                || (p1.0 <= 1.0 + TILE_BOUNDARY_EPSILON && p1.1 <= 1.0 + TILE_BOUNDARY_EPSILON)
            {
                y_edge = p0.1;
            }
            p0.0 = EPSILON;
        } else {
            y_edge = p0.1;
        }
    } else if p1.0 == 0.0 {
        if p1.1 == 0.0 {
            if keep_horizontal_tile_edges && p0.1 == 0.0 {
                y_edge = p1.1;
            }
            p1.0 = EPSILON;
        } else {
            y_edge = p1.1;
        }
    }
    if p0.0 == p0.0.floor() && p0.0 != 0.0 {
        p0.0 -= EPSILON;
    }
    if p1.0 == p1.0.floor() && p1.0 != 0.0 {
        p1.0 -= EPSILON;
    }
    if !is_down {
        std::mem::swap(&mut p0, &mut p1);
    }

    LineSegment {
        point0: p0,
        point1: p1,
        y_edge,
    }
}

fn clamp_tile_coord(value: f32) -> f32 {
    let value = value.clamp(0.0, TILE_SIZE as f32);
    if value <= TILE_BOUNDARY_EPSILON {
        0.0
    } else if TILE_SIZE as f32 - value <= TILE_BOUNDARY_EPSILON {
        TILE_SIZE as f32
    } else {
        value
    }
}

fn clip_segment_to_tile(
    p0: [f32; 2],
    p1: [f32; 2],
    tile_min: [f32; 2],
    tile_max: [f32; 2],
) -> ([f32; 2], [f32; 2]) {
    let delta = [p1[0] - p0[0], p1[1] - p0[1]];
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let mut t0_clip = 0u8;
    let mut t1_clip = 0u8;

    if clip_range(
        -delta[0],
        p0[0] - tile_min[0],
        CLIP_LEFT,
        &mut t0,
        &mut t1,
        &mut t0_clip,
        &mut t1_clip,
    ) && clip_range(
        delta[0],
        tile_max[0] - p0[0],
        CLIP_RIGHT,
        &mut t0,
        &mut t1,
        &mut t0_clip,
        &mut t1_clip,
    ) && clip_range(
        -delta[1],
        p0[1] - tile_min[1],
        CLIP_TOP,
        &mut t0,
        &mut t1,
        &mut t0_clip,
        &mut t1_clip,
    ) && clip_range(
        delta[1],
        tile_max[1] - p0[1],
        CLIP_BOTTOM,
        &mut t0,
        &mut t1,
        &mut t0_clip,
        &mut t1_clip,
    ) {
        let mut clipped0 = [p0[0] + delta[0] * t0, p0[1] + delta[1] * t0];
        let mut clipped1 = [p0[0] + delta[0] * t1, p0[1] + delta[1] * t1];
        snap_clip_planes(&mut clipped0, t0_clip, tile_min, tile_max);
        snap_clip_planes(&mut clipped1, t1_clip, tile_min, tile_max);
        return (clipped0, clipped1);
    }

    (
        [
            p0[0].clamp(tile_min[0], tile_max[0]),
            p0[1].clamp(tile_min[1], tile_max[1]),
        ],
        [
            p1[0].clamp(tile_min[0], tile_max[0]),
            p1[1].clamp(tile_min[1], tile_max[1]),
        ],
    )
}

const CLIP_LEFT: u8 = 1 << 0;
const CLIP_RIGHT: u8 = 1 << 1;
const CLIP_TOP: u8 = 1 << 2;
const CLIP_BOTTOM: u8 = 1 << 3;

fn clip_range(
    p: f32,
    q: f32,
    plane: u8,
    t0: &mut f32,
    t1: &mut f32,
    t0_clip: &mut u8,
    t1_clip: &mut u8,
) -> bool {
    if p == 0.0 {
        return q >= 0.0;
    }
    let r = q / p;
    if p < 0.0 {
        if r > *t1 {
            return false;
        }
        if r > *t0 {
            *t0 = r;
            *t0_clip = plane;
        } else if r == *t0 {
            *t0_clip |= plane;
        }
    } else {
        if r < *t0 {
            return false;
        }
        if r < *t1 {
            *t1 = r;
            *t1_clip = plane;
        } else if r == *t1 {
            *t1_clip |= plane;
        }
    }
    true
}

fn snap_clip_planes(point: &mut [f32; 2], planes: u8, tile_min: [f32; 2], tile_max: [f32; 2]) {
    if planes & CLIP_LEFT != 0 {
        point[0] = tile_min[0];
    }
    if planes & CLIP_RIGHT != 0 {
        point[0] = tile_max[0];
    }
    if planes & CLIP_TOP != 0 {
        point[1] = tile_min[1];
    }
    if planes & CLIP_BOTTOM != 0 {
        point[1] = tile_max[1];
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use std::sync::atomic::AtomicU32;

    use super::{ScanCpuPipeline, ScanCpuPrepared};
    use crate::{
        Canvas,
        cpu::computes::cumsum::run_backdrop_cumsum,
        cpu::computes::fine::build_tile_alpha,
        shared::{
            bd_record::BackdropRecord, bounds::TileBbox, fill::FillRule, line::Line,
            line_seg::LineSegment, path::PathRecord, scan_line::plan_scan_line,
            tile_seg_range::TileSegmentRange,
        },
    };
    use peniko::{
        Color,
        kurbo::{Affine, Line as KurboLine, Shape, Stroke},
    };

    fn one_tile_backdrop_record(segment_capacity: u32) -> BackdropRecord {
        BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 1,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 1,
            tile_y1: 1,
            segment_start: 0,
            segment_capacity,
            segment_count: 0,
        }
    }

    fn scan_lines(
        lines: &[Line],
        bbox: TileBbox,
        segment_capacity: u32,
    ) -> (
        BackdropRecord,
        Vec<i32>,
        Vec<TileSegmentRange>,
        Vec<LineSegment>,
    ) {
        scan_lines_with_flags(lines, bbox, segment_capacity, 0)
    }

    fn scan_lines_with_flags(
        lines: &[Line],
        bbox: TileBbox,
        segment_capacity: u32,
        path_flags: u32,
    ) -> (
        BackdropRecord,
        Vec<i32>,
        Vec<TileSegmentRange>,
        Vec<LineSegment>,
    ) {
        let tile_count = bbox.tile_stride() * (bbox.y1 - bbox.y0);
        let path_records = [PathRecord {
            path_id: 0,
            line_count: lines.len() as u32,
            line_start: 0,
            flags: path_flags,
        }];
        let backdrop_record = BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: tile_count,
            tile_x0: bbox.x0,
            tile_y0: bbox.y0,
            tile_x1: bbox.x1,
            tile_y1: bbox.y1,
            segment_start: 0,
            segment_capacity,
            segment_count: 0,
        };
        let mut backdrops = vec![0; tile_count as usize];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); tile_count as usize];
        let mut segments = vec![LineSegment::default(); segment_capacity as usize];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0; tile_count as usize];
        let mut segment_tile_cursors = (0..tile_count).map(|_| AtomicU32::new(0)).collect();

        ScanCpuPipeline::new()
            .prepare(
                lines,
                &path_records,
                &[backdrop_record],
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (bbox.x1, bbox.y1),
            )
            .run();

        (backdrop_record, backdrops, tile_segment_ranges, segments)
    }

    #[test]
    fn plan_scan_line_rejects_zero_length_line() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };
        let line = Line {
            path_id: 0,
            _pad: 0.0,
            p0: [4.0, 4.0],
            p1: [4.0, 4.0],
        };

        assert!(plan_scan_line(line, bbox, false).is_none());
    }

    #[test]
    fn run_skips_zero_tile_backdrop_record() {
        let lines = [Line {
            path_id: 0,
            _pad: 0.0,
            p0: [0.0, 0.0],
            p1: [0.0, 16.0],
        }];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: 1,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 0,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 0,
            tile_y1: 0,
            segment_start: 0,
            segment_capacity: 0,
            segment_count: 0,
        }];
        let mut backdrops = Vec::new();
        let mut tile_segment_ranges = Vec::new();
        let mut segments = Vec::new();
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = Vec::new();
        let mut segment_tile_cursors = Vec::new();

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (1, 1),
            )
            .run();

        assert!(backdrops.is_empty());
        assert!(tile_segment_ranges.is_empty());
        assert!(segments.is_empty());
    }

    #[test]
    fn run_adds_backdrop_delta_for_line_left_of_tile_bbox() {
        let lines = [Line {
            path_id: 0,
            _pad: 0.0,
            p0: [-4.0, 0.0],
            p1: [-4.0, 16.0],
        }];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: 1,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [one_tile_backdrop_record(1)];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 1];
        let mut segments = vec![LineSegment::default(); 1];
        let mut segments_bump = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_counts = vec![0];
        let mut segment_tile_cursors = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (1, 1),
            )
            .run();

        assert_eq!(backdrops, vec![-1]);
        assert_eq!(segments_bump[0].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn run_emits_segment_for_line_inside_tile() {
        let lines = [Line {
            path_id: 0,
            _pad: 0.0,
            p0: [4.0, 0.0],
            p1: [4.0, 16.0],
        }];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: 1,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [one_tile_backdrop_record(1)];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 1];
        let mut segments = vec![LineSegment::default(); 1];
        let mut segments_bump = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_counts = vec![0];
        let mut segment_tile_cursors = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (1, 1),
            )
            .run();

        assert_eq!(backdrops, vec![0]);
        assert_eq!(segments_bump[0].load(Ordering::Relaxed), 1);
        assert_eq!(
            tile_segment_ranges[0],
            TileSegmentRange { start: 0, end: 1 }
        );
        assert!((segments[0].point0.0 - 4.0).abs() < 1e-3);
        assert!((segments[0].point1.0 - 4.0).abs() < 1e-3);
        assert!((segments[0].point0.1 - 0.0).abs() < 1e-6);
        assert!((segments[0].point1.1 - 16.0).abs() < 1e-6);
    }

    #[test]
    fn run_packs_segments_into_per_tile_ranges() {
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [4.0, 0.0],
                p1: [20.0, 16.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [20.0, 0.0],
                p1: [4.0, 16.0],
            },
        ];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: 2,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 2,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 2,
            tile_y1: 1,
            segment_start: 0,
            segment_capacity: 4,
            segment_count: 0,
        }];
        let mut backdrops = vec![0; 2];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 2];
        let mut segments = vec![LineSegment::default(); 4];
        let mut segments_bump = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_counts = vec![0; 2];
        let mut segment_tile_cursors = vec![0; 2]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (2, 1),
            )
            .run();

        assert_eq!(segments_bump[0].load(Ordering::Relaxed), 4);
        assert_eq!(
            tile_segment_ranges,
            vec![
                TileSegmentRange { start: 0, end: 2 },
                TileSegmentRange { start: 2, end: 4 },
            ]
        );
        assert!(
            segments[0..2]
                .iter()
                .all(|segment| { segment.point0.0 <= 16.0 && segment.point1.0 <= 16.0 })
        );
        assert!(
            segments[2..4]
                .iter()
                .all(|segment| { segment.point0.0 >= 0.0 && segment.point1.0 >= 0.0 })
        );
    }

    #[test]
    fn run_adds_top_clipped_backdrop_bump_before_cumsum() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 8,
            y1: 3,
        };
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [20.0, -8.0],
                p1: [20.0, 40.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [100.0, 40.0],
                p1: [100.0, -8.0],
            },
        ];

        let (record, mut backdrops, _, _) = scan_lines(&lines, bbox, 16);

        assert_eq!(&backdrops[0..8], &[0, 0, -1, 0, 0, 0, 0, 1]);
        assert_eq!(&backdrops[8..16], &[0, 0, -1, 0, 0, 0, 0, 1]);
        run_backdrop_cumsum(&mut backdrops, &[record]);
        for row in backdrops.chunks_exact(8) {
            assert_eq!(row, &[0, 0, -1, -1, -1, -1, -1, 0]);
        }
    }

    #[test]
    fn plan_top_clipped_bump_snaps_exact_boundary_without_reprocessing_top_edge() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 8,
            y1: 2,
        };
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [32.0, -8.0],
                p1: [32.0, 32.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [96.0, 32.0],
                p1: [96.0, -8.0],
            },
        ];

        assert_eq!(
            plan_scan_line(lines[0], bbox, false)
                .unwrap()
                .top_clip_bump_x,
            Some(2)
        );
        assert_eq!(
            plan_scan_line(lines[1], bbox, false)
                .unwrap()
                .top_clip_bump_x,
            Some(6)
        );

        let line_on_top = Line {
            path_id: 0,
            _pad: 0.0,
            p0: [32.0, 0.0],
            p1: [32.0, 32.0],
        };
        assert_eq!(
            plan_scan_line(line_on_top, bbox, false)
                .unwrap()
                .top_clip_bump_x,
            None
        );
    }

    #[test]
    fn plan_top_clipped_bump_ignores_endpoint_on_clip_top() {
        let bbox = TileBbox {
            x0: 1,
            y0: 0,
            x1: 17,
            y1: 8,
        };
        let line = Line {
            path_id: 0,
            _pad: 0.0,
            p0: [30.156_143, -6.175_184_2],
            p1: [30.0, 0.0],
        };

        assert_eq!(
            plan_scan_line(line, bbox, false).unwrap().top_clip_bump_x,
            None
        );
    }

    #[test]
    fn run_cancels_top_clipped_stroke_cap_on_tile_boundary() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 11,
            y1: 1,
        };
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [159.862_64, -0.480_762],
                p1: [19.862_64, 39.519_238],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [160.137_36, 0.480_762],
                p1: [159.862_64, -0.480_762],
            },
        ];

        let (_, backdrops, _, _) = scan_lines(&lines, bbox, 16);

        assert_eq!(&backdrops[0..11], &[0; 11]);
    }

    #[test]
    fn run_keeps_top_left_stroke_cap_out_of_backdrop() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 12,
            y1: 12,
        };
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [1.494_818_7, -1.328_727_7],
                p1: [161.494_81, 178.671_28],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [161.494_81, 178.671_28],
                p1: [158.505_19, 181.328_72],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [158.505_19, 181.328_72],
                p1: [-1.494_818_7, 1.328_727_7],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-1.494_818_7, 1.328_727_7],
                p1: [1.494_818_7, -1.328_727_7],
            },
        ];

        let (record, mut backdrops, _, _) = scan_lines(&lines, bbox, 256);

        assert_eq!(&backdrops[0..12], &[0; 12]);
        run_backdrop_cumsum(&mut backdrops, &[record]);
        assert_eq!(&backdrops[0..12], &[0; 12]);
    }

    #[test]
    fn run_does_not_duplicate_existing_top_edge_bump() {
        let bbox = TileBbox {
            x0: 0,
            y0: 0,
            x1: 8,
            y1: 2,
        };
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [32.0, 0.0],
                p1: [32.0, 32.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [96.0, 32.0],
                p1: [96.0, 0.0],
            },
        ];

        let (_, backdrops, _, _) = scan_lines(&lines, bbox, 16);

        assert_eq!(&backdrops[0..8], &[0, 0, 0, -1, 0, 0, 0, 1]);
        assert_eq!(&backdrops[8..16], &[0, 0, 0, -1, 0, 0, 0, 1]);
    }

    #[test]
    fn run_keeps_generated_horizontal_path_dash_backdrops_empty() {
        let mut scene = Canvas::new(1071, 651);
        scene.push_stroke(
            KurboLine::new((0.0, 216.5), (652.0, 216.5)).to_path(0.25),
            Stroke::new(1.0).with_dashes(0.0, [1.0_f64, 2.0_f64]),
            Color::BLACK,
            Affine::translate((387.0, 104.0)),
            FillRule::NonZero,
            0.25,
        );
        let record = scene.bd_records[0];
        let bbox = TileBbox {
            x0: record.tile_x0,
            y0: record.tile_y0,
            x1: record.tile_x1,
            y1: record.tile_y1,
        };

        let (record, mut backdrops, ranges, segments) = scan_lines_with_flags(
            &scene.lines,
            bbox,
            scene.tile_cnt,
            scene.path_records[0].flags,
        );
        run_backdrop_cumsum(&mut backdrops, &[record]);

        let mut first_row_coverage = 0usize;
        for tile_x in 0..bbox.tile_stride() {
            let range = ranges[tile_x as usize];
            let alpha = build_tile_alpha(
                &segments[range.start as usize..range.end as usize],
                backdrops[tile_x as usize],
                FillRule::NonZero,
            );
            first_row_coverage += alpha[..crate::TILE_SIZE as usize]
                .iter()
                .filter(|&&value| value != 0)
                .count();
            for row in 1..crate::TILE_SIZE as usize {
                let row_alpha =
                    &alpha[row * crate::TILE_SIZE as usize..(row + 1) * crate::TILE_SIZE as usize];
                assert!(
                    row_alpha.iter().all(|&value| value == 0),
                    "tile {} row {row} leaked below the 1px dash: {row_alpha:?}",
                    bbox.x0 + tile_x
                );
            }
        }
        assert!(
            first_row_coverage > 0,
            "expected generated dash stroke to cover its top pixel row"
        );
    }

    #[test]
    fn run_preserves_subtile_offsets_for_clipped_diagonal_stroke() {
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [1.494_818_7, -1.328_727_7],
                p1: [161.494_81, 178.671_28],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [161.494_81, 178.671_28],
                p1: [158.505_19, 181.328_72],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [158.505_19, 181.328_72],
                p1: [-1.494_818_7, 1.328_727_7],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-1.494_818_7, 1.328_727_7],
                p1: [1.494_818_7, -1.328_727_7],
            },
        ];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: lines.len() as u32,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 12 * 12,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 12,
            tile_y1: 12,
            segment_start: 0,
            segment_capacity: 256,
            segment_count: 0,
        }];
        let mut backdrops = vec![0; 12 * 12];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 12 * 12];
        let mut segments = vec![LineSegment::default(); 256];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0; 12 * 12];
        let mut segment_tile_cursors = (0..12 * 12).map(|_| AtomicU32::new(0)).collect();

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (13, 13),
            )
            .run();

        let tile_ix = 2 * 12 + 2;
        let range = tile_segment_ranges[tile_ix];
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[tile_ix],
            FillRule::NonZero,
        );

        assert!(segments_bump[0].load(Ordering::Relaxed) >= 2);
        assert_eq!(range.end - range.start, 2);
        assert_eq!(alpha[4 * 16], 255);
    }

    #[test]
    fn run_snaps_clipped_segments_to_tile_boundaries() {
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [0.560_557, -0.498_273],
                p1: [240.560_56, 269.501_74],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [240.560_56, 269.501_74],
                p1: [239.439_44, 270.498_26],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [239.439_44, 270.498_26],
                p1: [-0.560_557, 0.498_273],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-0.560_557, 0.498_273],
                p1: [0.560_557, -0.498_273],
            },
        ];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: lines.len() as u32,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 19 * 19,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 19,
            tile_y1: 19,
            segment_start: 0,
            segment_capacity: 512,
            segment_count: 0,
        }];
        let mut backdrops = vec![0; 19 * 19];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 19 * 19];
        let mut segments = vec![LineSegment::default(); 512];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0; 19 * 19];
        let mut segment_tile_cursors = (0..19 * 19).map(|_| AtomicU32::new(0)).collect();

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (19, 19),
            )
            .run();

        let tile_ix = 2 * 19 + 2;
        let range = tile_segment_ranges[tile_ix];
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[tile_ix],
            FillRule::NonZero,
        );

        assert!(segments_bump[0].load(Ordering::Relaxed) >= 2);
        assert_eq!(range.end - range.start, 2);
        assert!(alpha[4 * 16] > 0);
        assert_eq!(alpha[4 * 16 + 15], 0);
    }

    #[test]
    fn run_keeps_right_clipped_skew_edge_from_filling_tile() {
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-90.0, 0.0],
                p1: [90.0, 0.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [90.0, 0.0],
                p1: [304.515_66, 180.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [304.515_66, 180.0],
                p1: [124.515_66, 180.0],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [124.515_66, 180.0],
                p1: [-90.0, 0.0],
            },
        ];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: lines.len() as u32,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 12 * 12,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 12,
            tile_y1: 12,
            segment_start: 0,
            segment_capacity: 512,
            segment_count: 0,
        }];
        let mut backdrops = vec![0; 12 * 12];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 12 * 12];
        let mut segments = vec![LineSegment::default(); 512];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0; 12 * 12];
        let mut segment_tile_cursors = (0..12 * 12).map(|_| AtomicU32::new(0)).collect();

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (12, 12),
            )
            .run();
        run_backdrop_cumsum(&mut backdrops, &backdrop_records);

        let tile_ix = 4 * 12 + 10;
        let range = tile_segment_ranges[tile_ix];
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[tile_ix],
            FillRule::NonZero,
        );

        assert_eq!(alpha[15], 0);
        assert_eq!(alpha[16 + 15], 0);
    }

    #[test]
    fn run_keeps_top_left_clipped_cap_from_filling_tile() {
        let lines = [
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-0.137_36, -0.480_762],
                p1: [0.137_36, 0.480_762],
            },
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [0.137_36, 0.480_762],
                p1: [-139.862_64, 40.480_762],
            },
        ];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: lines.len() as u32,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [one_tile_backdrop_record(16)];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default()];
        let mut segments = vec![LineSegment::default(); 16];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0];
        let mut segment_tile_cursors = vec![AtomicU32::new(0)];

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (1, 1),
            )
            .run();

        let range = tile_segment_ranges[0];
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[0],
            FillRule::NonZero,
        );

        assert_eq!(backdrops, vec![0]);
        assert_eq!(range.end - range.start, 2);
        assert!(alpha[0] > 0);
        assert_eq!(alpha[1], 0);
        assert_eq!(alpha[16], 0);
    }

    #[test]
    fn run_keeps_long_top_left_corner_crossing_on_top_edge() {
        let lines = [Line {
            path_id: 0,
            _pad: 0.0,
            p0: [15.0, 15.0],
            p1: [150.0, 150.0],
        }];
        let path_records = [PathRecord {
            path_id: 0,
            line_count: 1,
            line_start: 0,
            flags: 0,
        }];
        let backdrop_records = [BackdropRecord {
            path_id: 0,
            data_offset: 0,
            data_len: 1,
            tile_x0: 2,
            tile_y0: 2,
            tile_x1: 3,
            tile_y1: 3,
            segment_start: 0,
            segment_capacity: 1,
            segment_count: 0,
        }];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default()];
        let mut segments = vec![LineSegment::default()];
        let mut segments_bump = vec![AtomicU32::new(0)];
        let mut segment_tile_counts = vec![0];
        let mut segment_tile_cursors = vec![AtomicU32::new(0)];

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &path_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (12, 12),
            )
            .run();

        let range = tile_segment_ranges[0];
        assert_eq!(range, TileSegmentRange { start: 0, end: 1 });
        assert!(
            segments[0].y_edge > 1.0e8,
            "long top-left corner crossings must not be counted as left-edge crossings: {:?}",
            segments[0]
        );
    }

    #[test]
    fn fill_ranges_from_counts_parallel_builds_prefix_sum() {
        let mut ranges = vec![TileSegmentRange::default(); 8];
        let counts = [3u32, 0, 2, 1, 4, 0, 0, 2]
            .into_iter()
            .map(AtomicU32::new)
            .collect::<Vec<_>>();

        ScanCpuPrepared::fill_ranges_from_counts_parallel(&mut ranges, &counts, 10);

        assert_eq!(
            ranges,
            vec![
                TileSegmentRange { start: 10, end: 13 },
                TileSegmentRange { start: 13, end: 13 },
                TileSegmentRange { start: 13, end: 15 },
                TileSegmentRange { start: 15, end: 16 },
                TileSegmentRange { start: 16, end: 20 },
                TileSegmentRange { start: 20, end: 20 },
                TileSegmentRange { start: 20, end: 20 },
                TileSegmentRange { start: 20, end: 22 },
            ]
        );
    }
}
