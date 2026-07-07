use std::sync::atomic::{AtomicU32, Ordering};

use rayon::prelude::*;

use crate::{
    TILE_SIZE,
    shared::{
        bounds::TileBbox,
        line::Line,
        line_seg::LineSegment,
        path::PathRecord,
        scan_line::{
            SCAN_EPSILON, ScanLinePlan, ScannedTile, for_each_scanned_tile, plan_scan_line,
        },
        tile_seg_range::TileSegmentRange,
    },
};

// DDA-derived top/bottom clips are nudged into the tile before y_edge handling.
// This is not a comparison tolerance: at global pixel coordinates, a 1e-6 offset can
// round back to the boundary in f32 and be misclassified as a left-edge crossing.
const TILE_CLIP_NUDGE: f32 = 1.0e-3;

/// wgpu compute scan_assign: one workgroup per path.
pub struct ScanCpuPipeline {}

pub struct ScanCpuPrepared<'a> {
    lines: &'a [Line],
    path_records: &'a [PathRecord],
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
        (0..self.path_records.len())
            .into_par_iter()
            .for_each(|record_ix| {
                let path_record = &self.path_records[record_ix];
                let path_id = path_record.path_id as usize;
                let bbox = TileBbox {
                    x0: path_record.tile_x0,
                    y0: path_record.tile_y0,
                    x1: path_record.tile_x1,
                    y1: path_record.tile_y1,
                };
                let tile_count = path_record.data_len as usize;
                let data_offset = path_record.data_offset as usize;
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
                for &line in lines {
                    if let Some(plan) = plan_scan_line(line, bbox) {
                        for y in plan.ymin..plan.ymax {
                            let base = ((y - bbox.y0 as i32) * bbox.tile_stride() as i32) as usize;
                            backdrop[base] += plan.delta;
                        }
                        for_each_scanned_tile(&plan, bbox, self.tiles_size, |tile| {
                            if tile.top_edge && tile.x + 1 < bbox.x1 as i32 {
                                let x_bump = (tile.x + 1).max(bbox.x0 as i32);
                                let bump_ix =
                                    ((tile.y - bbox.y0 as i32) * bbox.tile_stride() as i32 + x_bump
                                        - bbox.x0 as i32)
                                        as usize;
                                backdrop[bump_ix] += plan.delta;
                            }

                            let local_ix =
                                Self::local_tile_ix(path_record, tile.global_ix, self.tiles_size.0);
                            let count = counts[local_ix];
                            counts[local_ix] = count + 1;
                        });
                    }
                }

                let cursors = &self.segment_tile_cursors[data_offset..data_offset + tile_count];
                let mut next = path_record.segment_start;
                for ((range, count), cursor) in ranges.iter_mut().zip(counts).zip(cursors) {
                    let count = *count;
                    range.start = next;
                    next += count;
                    range.end = next;
                    cursor.store(range.start, Ordering::Relaxed);
                }

                let segment_count = next - path_record.segment_start;
                self.segments_bump[path_id].store(segment_count, Ordering::Relaxed);
                debug_assert!(
                    segment_count <= path_record.segment_capacity,
                    "scan emitted more segments than reserved capacity"
                );

                if segment_count == 0 {
                    return;
                }

                let segments_ptr = segments as *mut LineSegment;
                let segments =
                    unsafe { std::slice::from_raw_parts_mut(segments_ptr, self.segments.len()) };

                for &line in lines {
                    if let Some(plan) = plan_scan_line(line, bbox) {
                        for_each_scanned_tile(&plan, bbox, self.tiles_size, |tile| {
                            let segment = clip_line_to_tile(&plan, tile);
                            let local_ix =
                                Self::local_tile_ix(path_record, tile.global_ix, self.tiles_size.0);
                            let dst = cursors[local_ix].load(Ordering::Relaxed);
                            cursors[local_ix].store(dst + 1, Ordering::Relaxed);
                            debug_assert!(
                                dst < path_record.segment_start + path_record.segment_capacity
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

    fn local_tile_ix(path_record: &PathRecord, tile_id: u32, tiles_width: u32) -> usize {
        let tile_x = tile_id % tiles_width;
        let tile_y = tile_id / tiles_width;
        let local_x = tile_x - path_record.tile_x0;
        let local_y = tile_y - path_record.tile_y0;
        let stride = path_record.tile_x1 - path_record.tile_x0;
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

fn clip_line_to_tile(plan: &ScanLinePlan, tile: ScannedTile) -> LineSegment {
    let tile_xy = [
        tile.x as f32 * TILE_SIZE as f32,
        tile.y as f32 * TILE_SIZE as f32,
    ];
    let tile_xy1 = [tile_xy[0] + TILE_SIZE as f32, tile_xy[1] + TILE_SIZE as f32];

    let mut xy0 = plan.xy0;
    let mut xy1 = plan.xy1;
    if tile.sub_ix > 0 {
        let z_prev = (plan.a * (tile.sub_ix as f32 - 1.0) + plan.b).floor();
        if tile.z == z_prev {
            let mut x = x_at_y(plan.xy0, plan.xy1, tile_xy[1]);
            x = x.clamp(tile_xy[0] + TILE_CLIP_NUDGE, tile_xy1[0]);
            xy0 = [x, tile_xy[1]];
        } else {
            let x_clip = if plan.sign > 0.0 {
                tile_xy[0]
            } else {
                tile_xy1[0]
            };
            let y = clip_y_at_x(plan.xy0, plan.xy1, x_clip, tile_xy[1], tile_xy1[1]);
            xy0 = [x_clip, y];
        }
    }
    if tile.sub_ix < plan.count - 1 {
        let z_next = (plan.a * (tile.sub_ix as f32 + 1.0) + plan.b).floor();
        if tile.z == z_next {
            let mut x = x_at_y(plan.xy0, plan.xy1, tile_xy1[1]);
            x = x.clamp(tile_xy[0] + TILE_CLIP_NUDGE, tile_xy1[0]);
            xy1 = [x, tile_xy1[1]];
        } else {
            let x_clip = if plan.sign > 0.0 {
                tile_xy1[0]
            } else {
                tile_xy[0]
            };
            let y = clip_y_at_x(plan.xy0, plan.xy1, x_clip, tile_xy[1], tile_xy1[1]);
            xy1 = [x_clip, y];
        }
    }

    let mut y_edge = 1e9f32;
    let mut p0 = (
        clamp_tile_coord(xy0[0] - tile_xy[0]),
        clamp_tile_coord(xy0[1] - tile_xy[1]),
    );
    let mut p1 = (
        clamp_tile_coord(xy1[0] - tile_xy[0]),
        clamp_tile_coord(xy1[1] - tile_xy[1]),
    );
    if p0.0 == 0.0 {
        if p1.0 == 0.0 {
            p0.0 = SCAN_EPSILON;
            if p0.1 == 0.0 {
                p1.0 = SCAN_EPSILON;
                p1.1 = TILE_SIZE as f32;
            } else {
                p1.0 = 2.0 * SCAN_EPSILON;
                p1.1 = p0.1;
            }
        } else if p0.1 == 0.0 {
            p0.0 = SCAN_EPSILON;
        } else {
            y_edge = p0.1;
        }
    } else if p1.0 == 0.0 {
        if p1.1 == 0.0 {
            p1.0 = SCAN_EPSILON;
        } else {
            y_edge = p1.1;
        }
    }
    if p0.0 == p0.0.floor() && p0.0 != 0.0 {
        p0.0 -= SCAN_EPSILON;
    }
    if p1.0 == p1.0.floor() && p1.0 != 0.0 {
        p1.0 -= SCAN_EPSILON;
    }
    if !plan.is_down {
        std::mem::swap(&mut p0, &mut p1);
    }

    LineSegment {
        p0x: p0.0,
        p0y: p0.1,
        p1x: p1.0,
        p1y: p1.1,
        y_edge,
    }
}

fn x_at_y(p0: [f32; 2], p1: [f32; 2], y: f32) -> f32 {
    p0[0] + (p1[0] - p0[0]) * ((y - p0[1]) / (p1[1] - p0[1]))
}

fn y_at_x(p0: [f32; 2], p1: [f32; 2], x: f32) -> f32 {
    p0[1] + (p1[1] - p0[1]) * ((x - p0[0]) / (p1[0] - p0[0]))
}

fn clip_y_at_x(p0: [f32; 2], p1: [f32; 2], x: f32, tile_y0: f32, tile_y1: f32) -> f32 {
    let y = y_at_x(p0, p1, x);
    if y <= tile_y0 + SCAN_EPSILON {
        let top_x = x_at_y(p0, p1, tile_y0);
        if (top_x - x).abs() > TILE_CLIP_NUDGE {
            tile_y0
        } else {
            tile_y0 + TILE_CLIP_NUDGE
        }
    } else {
        y.clamp(tile_y0 + TILE_CLIP_NUDGE, tile_y1)
    }
}

fn clamp_tile_coord(value: f32) -> f32 {
    let value = value.clamp(0.0, TILE_SIZE as f32);
    if value <= SCAN_EPSILON {
        0.0
    } else if TILE_SIZE as f32 - value <= SCAN_EPSILON {
        TILE_SIZE as f32
    } else {
        value
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
            bounds::TileBbox,
            fill::FillRule,
            line::Line,
            line_seg::LineSegment,
            path::PathRecord,
            scan_line::{for_each_scanned_tile, plan_scan_line},
            tile_seg_range::TileSegmentRange,
        },
    };
    use peniko::{
        Color,
        kurbo::{Affine, BezPath, Line as KurboLine, Shape, Stroke},
    };

    fn one_tile_path_record(line_count: u32, segment_capacity: u32) -> PathRecord {
        PathRecord {
            path_id: 0,
            line_count,
            line_start: 0,
            flags: 0,
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

    fn path_record_for_lines(
        line_count: u32,
        bbox: TileBbox,
        segment_capacity: u32,
        flags: u32,
    ) -> PathRecord {
        let tile_count = bbox.tile_stride() * (bbox.y1 - bbox.y0);
        PathRecord {
            path_id: 0,
            line_count,
            line_start: 0,
            flags,
            data_offset: 0,
            data_len: tile_count,
            tile_x0: bbox.x0,
            tile_y0: bbox.y0,
            tile_x1: bbox.x1,
            tile_y1: bbox.y1,
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
        PathRecord,
        Vec<i32>,
        Vec<TileSegmentRange>,
        Vec<LineSegment>,
    ) {
        let tile_count = bbox.tile_stride() * (bbox.y1 - bbox.y0);
        let backdrop_record = path_record_for_lines(lines.len() as u32, bbox, segment_capacity, 0);
        let path_records = [backdrop_record];
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

        assert!(plan_scan_line(line, bbox).is_none());
    }

    #[test]
    fn run_skips_zero_tile_backdrop_record() {
        let lines = [Line {
            path_id: 0,
            _pad: 0.0,
            p0: [0.0, 0.0],
            p1: [0.0, 16.0],
        }];
        let path_records = [path_record_for_lines(
            1,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
            0,
            0,
        )];
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
        let path_records = [one_tile_path_record(1, 1)];
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
        let path_records = [one_tile_path_record(1, 1)];
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
        assert!((segments[0].p0x - 4.0).abs() < 1e-3);
        assert!((segments[0].p1x - 4.0).abs() < 1e-3);
        assert!((segments[0].p0y - 0.0).abs() < 1e-6);
        assert!((segments[0].p1y - 16.0).abs() < 1e-6);
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
        let path_records = [path_record_for_lines(
            2,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 2,
                y1: 1,
            },
            4,
            0,
        )];
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
                .all(|segment| { segment.p0x <= 16.0 && segment.p1x <= 16.0 })
        );
        assert!(
            segments[2..4]
                .iter()
                .all(|segment| { segment.p0x >= 0.0 && segment.p1x >= 0.0 })
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
    fn scan_line_initial_top_edge_requires_actual_tile_boundary() {
        let bbox = TileBbox {
            x0: 51,
            y0: 30,
            x1: 53,
            y1: 31,
        };
        let p0 = [832.75, 484.75];
        let first_tile = |p1| {
            let plan = plan_scan_line(
                Line {
                    path_id: 0,
                    _pad: 0.0,
                    p0,
                    p1,
                },
                bbox,
            )
            .unwrap();
            let mut tiles = Vec::new();
            for_each_scanned_tile(&plan, bbox, (64, 64), |tile| {
                tiles.push((tile.x, tile.y, tile.top_edge));
            });
            tiles[0]
        };

        assert_eq!(first_tile([826.0, 480.0]), (51, 30, true));
        assert_eq!(first_tile([826.0, 480.000_03]), (51, 30, false));
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
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [19.862_64, 39.519_238],
                p1: [160.137_36, 0.480_762],
            },
        ];

        let (record, mut backdrops, tile_segment_ranges, segments) = scan_lines(&lines, bbox, 32);

        run_backdrop_cumsum(&mut backdrops, &[record]);
        let tile_ix = 10;
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

        let (record, mut backdrops, tile_segment_ranges, segments) = scan_lines(&lines, bbox, 256);

        run_backdrop_cumsum(&mut backdrops, &[record]);
        for tile_ix in 0..2 {
            let range = tile_segment_ranges[tile_ix];
            let alpha = build_tile_alpha(
                &segments[range.start as usize..range.end as usize],
                backdrops[tile_ix],
                FillRule::NonZero,
            );
            assert_eq!(alpha[15], 0, "tile {tile_ix}");
            assert_eq!(alpha[16 + 15], 0, "tile {tile_ix}");
        }
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
        let mut canvas = Canvas::new(1071, 651, 1.0);
        canvas.push_stroke(
            KurboLine::new((0.0, 216.5), (652.0, 216.5)).to_path(0.25),
            Stroke::new(1.0).with_dashes(0.0, [1.0_f64, 2.0_f64]),
            Color::BLACK,
            Affine::translate((387.0, 104.0)),
            FillRule::NonZero,
            0.25,
        );
        let record = canvas.path_records[0];
        let bbox = TileBbox {
            x0: record.tile_x0,
            y0: record.tile_y0,
            x1: record.tile_x1,
            y1: record.tile_y1,
        };
        let (record, mut backdrops, ranges, segments) =
            scan_lines(&canvas.lines, bbox, canvas.tile_cnt);
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
    fn run_keeps_local_top_edge_for_tall_stroke_outline() {
        let mut path = BezPath::new();
        path.move_to((39.0, 124.0));
        path.curve_to((39.0, 124.0), (38.8, 122.2), (40.6, 123.0));
        path.curve_to((42.4, 123.8), (164.6, 85.2), (188.2, 117.6));
        path.curve_to((188.2, 117.6), (174.8, 93.0), (39.0, 124.0));
        path.close_path();

        let transform = Affine::translate((331.0, 124.0))
            * Affine::scale_non_uniform(0.957_777_777_777_777_7, 0.597_777_777_777_777_7)
            * Affine::new([1.765_646_3, 0.0, 0.0, 1.765_646_3, 324.907_16, 255.009_42]);
        let mut canvas = Canvas::new(1248, 628, 1.0);
        canvas.push_stroke(
            path,
            Stroke::new(0.1),
            Color::BLACK,
            transform,
            FillRule::NonZero,
            0.25,
        );
        let record = canvas.path_records[0];
        let bbox = TileBbox {
            x0: record.tile_x0,
            y0: record.tile_y0,
            x1: record.tile_x1,
            y1: record.tile_y1,
        };

        let (record, mut backdrops, ranges, segments) =
            scan_lines(&canvas.lines, bbox, canvas.tile_cnt);
        run_backdrop_cumsum(&mut backdrops, &[record]);

        let tile_ix = ((25 - bbox.y0) * bbox.tile_stride() + 47 - bbox.x0) as usize;
        let range = ranges[tile_ix];
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[tile_ix],
            FillRule::NonZero,
        );

        assert!(
            alpha[..2 * crate::TILE_SIZE as usize]
                .iter()
                .any(|&a| a > 0)
        );
        for row in 2..crate::TILE_SIZE as usize {
            let row_alpha =
                &alpha[row * crate::TILE_SIZE as usize..(row + 1) * crate::TILE_SIZE as usize];
            assert!(
                row_alpha.iter().all(|&value| value == 0),
                "row {row} leaked below the local top-edge stroke: {row_alpha:?}"
            );
        }
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
        let path_records = [path_record_for_lines(
            lines.len() as u32,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 12,
                y1: 12,
            },
            256,
            0,
        )];
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
        let path_records = [path_record_for_lines(
            lines.len() as u32,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 19,
                y1: 19,
            },
            512,
            0,
        )];
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
        let path_records = [path_record_for_lines(
            lines.len() as u32,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 12,
                y1: 12,
            },
            512,
            0,
        )];
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
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                (12, 12),
            )
            .run();
        run_backdrop_cumsum(&mut backdrops, &path_records);

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
            Line {
                path_id: 0,
                _pad: 0.0,
                p0: [-139.862_64, 40.480_762],
                p1: [-0.137_36, -0.480_762],
            },
        ];
        let path_records = [one_tile_path_record(lines.len() as u32, 16)];
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
        run_backdrop_cumsum(&mut backdrops, &path_records);
        let alpha = build_tile_alpha(
            &segments[range.start as usize..range.end as usize],
            backdrops[0],
            FillRule::NonZero,
        );
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
        let path_records = [path_record_for_lines(
            1,
            TileBbox {
                x0: 2,
                y0: 2,
                x1: 3,
                y1: 3,
            },
            1,
            0,
        )];
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
