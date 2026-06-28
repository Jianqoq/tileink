use std::sync::atomic::{AtomicU32, Ordering};

use rayon::prelude::*;

use crate::{
    TILE_SCALE, TILE_SIZE,
    shared::{
        bd_record::BackdropRecord, bounds::TileBbox, line::Line, line_seg::LineSegment,
        path::PathRecord, tile_seg_range::TileSegmentRange,
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
                    if let Some(plan) = plan_scan_line(line, bbox) {
                        for_each_scanned_tile(&plan, bbox, self.tiles_size, |tile| {
                            let segment = clip_line_to_tile(
                                (plan.xy0, plan.xy1),
                                plan.is_down,
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

pub(crate) struct ScanLinePlan {
    xy0: [f32; 2],
    xy1: [f32; 2],
    is_down: bool,
    a: f32,
    b: f32,
    x0: f32,
    sign: f32,
    y0: f32,
    delta: i32,
    imin: u32,
    imax: u32,
    ymin: i32,
    ymax: i32,
}

#[derive(Clone, Copy)]
struct ScannedTile {
    x: i32,
    y: i32,
    global_ix: u32,
    top_edge: bool,
}

fn for_each_scanned_tile(
    plan: &ScanLinePlan,
    bbox: TileBbox,
    tiles_size: (u32, u32),
    mut f: impl FnMut(ScannedTile),
) {
    let mut last_z = (plan.a * (plan.imin as f32 - 1.0) + plan.b).floor();
    for i in plan.imin..plan.imax {
        let z = (plan.a * i as f32 + plan.b).floor();
        let y = (plan.y0 + i as f32 - z) as i32;
        let x = (plan.x0 + plan.sign * z) as i32;
        if y < bbox.y0 as i32 || y >= bbox.y1 as i32 || x < bbox.x0 as i32 || x >= bbox.x1 as i32 {
            last_z = z;
            continue;
        }

        let top_edge = if i == plan.imin {
            (plan.y0 - plan.xy0[1] * TILE_SCALE).abs() <= 1.0e-5
        } else {
            last_z == z
        };
        f(ScannedTile {
            x,
            y,
            global_ix: y as u32 * tiles_size.0 + x as u32,
            top_edge,
        });
        last_z = z;
    }
}

pub(crate) fn plan_scan_line(line: Line, bbox: TileBbox) -> Option<ScanLinePlan> {
    let p0 = line.p0;
    let p1 = line.p1;
    let is_down = p1[1] >= p0[1];
    let (xy0, xy1) = if is_down { (p0, p1) } else { (p1, p0) };

    let s0 = (xy0[0] * TILE_SCALE, xy0[1] * TILE_SCALE);
    let s1 = (xy1[0] * TILE_SCALE, xy1[1] * TILE_SCALE);
    let count_x = span(s0.0, s1.0) - 1;
    let count = count_x + span(s0.1, s1.1);

    let dx = (s1.0 - s0.0).abs();
    let dy = s1.1 - s0.1;
    if dx + dy == 0.0 {
        return None;
    }
    if dy == 0.0 && s0.1.floor() == s0.1 {
        return None;
    }

    let idxdy = 1.0 / (dx + dy);
    let mut a = dx * idxdy;
    let is_positive_slope = s1.0 >= s0.0;
    let sign = if is_positive_slope { 1.0 } else { -1.0 };
    let xt0 = (s0.0 * sign).floor();
    let c = s0.0 * sign - xt0;
    let y0 = s0.1.floor();
    let ytop = if s0.1 == s1.1 { s0.1.ceil() } else { y0 + 1.0 };
    let b = ((dy * c + dx * (ytop - s0.1)) * idxdy).min(0.999_999_94);
    let robust_err = (a * (count as f32 - 1.0) + b).floor() - count_x as f32;
    if robust_err != 0.0 {
        a -= 2e-7_f32.copysign(robust_err);
    }
    let x0 = xt0 * sign + if is_positive_slope { 0.0 } else { -1.0 };

    let xmin = s0.0.min(s1.0);
    if s0.1 >= bbox.y1 as f32 || s1.1 < bbox.y0 as f32 || xmin >= bbox.x1 as f32 {
        return None;
    }

    let mut imin = 0u32;
    if s0.1 < bbox.y0 as f32 {
        let mut iminf = ((bbox.y0 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + iminf - (a * iminf + b).floor() < bbox.y0 as f32 {
            iminf += 1.0;
        }
        imin = iminf as u32;
    }
    let mut imax = count;
    if s1.1 > bbox.y1 as f32 {
        let mut imaxf = ((bbox.y1 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + imaxf - (a * imaxf + b).floor() < bbox.y1 as f32 {
            imaxf += 1.0;
        }
        imax = imaxf as u32;
    }

    let delta = if is_down { -1 } else { 1 };
    let mut ymin = 0i32;
    let mut ymax = 0i32;
    if s0.0.max(s1.0) <= bbox.x0 as f32 {
        ymin = s0.1.ceil() as i32;
        ymax = s1.1.ceil() as i32;
        imax = imin;
    } else {
        let fudge = if is_positive_slope { 0.0 } else { 1.0 };
        if xmin < bbox.x0 as f32 {
            let mut f = ((sign * (bbox.x0 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox.x0 as f32) == is_positive_slope {
                f += 1.0;
            }
            let ynext = (y0 + f - (a * f + b).floor() + 1.0) as i32;
            if is_positive_slope {
                if f as u32 > imin {
                    ymin = (y0 + if y0 == s0.1 { 0.0 } else { 1.0 }) as i32;
                    ymax = ynext;
                    imin = f as u32;
                }
            } else if (f as u32) < imax {
                ymin = ynext;
                ymax = s1.1.ceil() as i32;
                imax = f as u32;
            }
        }
        if s0.0.max(s1.0) > bbox.x1 as f32 {
            let mut f = ((sign * (bbox.x1 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox.x1 as f32) == is_positive_slope {
                f += 1.0;
            }
            if is_positive_slope {
                imax = imax.min(f as u32);
            } else {
                imin = imin.max(f as u32);
            }
        }
    }
    imax = imin.max(imax);
    ymin = ymin.max(bbox.y0 as i32);
    ymax = ymax.min(bbox.y1 as i32);

    Some(ScanLinePlan {
        xy0,
        xy1,
        is_down,
        a,
        b,
        x0,
        sign,
        y0,
        delta,
        imin,
        imax,
        ymin,
        ymax,
    })
}

fn clip_line_to_tile(
    line: ([f32; 2], [f32; 2]),
    is_down: bool,
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
            p0.0 = EPSILON;
        } else {
            y_edge = p0.1;
        }
    } else if p1.0 == 0.0 {
        if p1.1 == 0.0 {
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

const TILE_BOUNDARY_EPSILON: f32 = 1.0e-4;

fn clip_segment_to_tile(
    p0: [f32; 2],
    p1: [f32; 2],
    tile_min: [f32; 2],
    tile_max: [f32; 2],
) -> ([f32; 2], [f32; 2]) {
    let delta = [p1[0] - p0[0], p1[1] - p0[1]];
    let mut t0 = 0.0;
    let mut t1 = 1.0;

    if clip_range(-delta[0], p0[0] - tile_min[0], &mut t0, &mut t1)
        && clip_range(delta[0], tile_max[0] - p0[0], &mut t0, &mut t1)
        && clip_range(-delta[1], p0[1] - tile_min[1], &mut t0, &mut t1)
        && clip_range(delta[1], tile_max[1] - p0[1], &mut t0, &mut t1)
    {
        return (
            [p0[0] + delta[0] * t0, p0[1] + delta[1] * t0],
            [p0[0] + delta[0] * t1, p0[1] + delta[1] * t1],
        );
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

fn clip_range(p: f32, q: f32, t0: &mut f32, t1: &mut f32) -> bool {
    if p == 0.0 {
        return q >= 0.0;
    }
    let r = q / p;
    if p < 0.0 {
        if r > *t1 {
            return false;
        }
        *t0 = (*t0).max(r);
    } else {
        if r < *t0 {
            return false;
        }
        *t1 = (*t1).min(r);
    }
    true
}

fn span(a: f32, b: f32) -> u32 {
    let hi = a.max(b).ceil();
    let lo = a.min(b).floor();
    (hi - lo).max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use std::sync::atomic::AtomicU32;

    use super::{ScanCpuPipeline, ScanCpuPrepared, plan_scan_line};
    use crate::{
        cpu::computes::fine::build_tile_alpha,
        shared::{
            bd_record::BackdropRecord, bounds::TileBbox, fill::FillRule, line::Line,
            line_seg::LineSegment, path::PathRecord, tile_seg_range::TileSegmentRange,
        },
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
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
