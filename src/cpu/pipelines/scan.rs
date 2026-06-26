use std::sync::atomic::{AtomicU32, Ordering};

use rayon::prelude::*;

use crate::{
    TILE_SCALE, TILE_SIZE,
    shared::{
        bd_record::BackdropRecord, bounds::TileBbox, draw_record::DrawRecord, line::Line,
        line_seg::LineSegment, pixel::TileMask, tile_seg_range::TileSegmentRange,
    },
};

/// wgpu compute scan_assign: one workgroup per path.
pub struct ScanCpuPipeline {}

pub struct ScanCpuPrepared<'a> {
    lines: &'a [Line],
    draw_records: &'a [DrawRecord],
    backdrop_records: &'a [BackdropRecord],
    backdrops: &'a mut Vec<i32>, // [path_id][tile_y][tile_x]
    tile_segment_ranges: &'a mut Vec<TileSegmentRange>,
    segments: &'a mut Vec<LineSegment>,
    segments_bump: &'a mut Vec<AtomicU32>,
    segment_tile_counts: &'a mut Vec<AtomicU32>,
    segment_tile_cursors: &'a mut Vec<AtomicU32>,
    packed_segments: &'a mut Vec<LineSegment>,
    tiles_size: (u32, u32),
}

impl<'a> ScanCpuPrepared<'a> {
    pub fn run(&mut self) {
        let backdrops = self.backdrops.as_mut_ptr() as *mut Vec<i32> as usize;
        let segments = self.segments.as_mut_ptr() as *mut LineSegment as usize;
        (0..self.lines.len()).into_par_iter().for_each(|line_id| {
            let line = self.lines[line_id];
            let backdrop_ptr = backdrops as *mut i32;
            let backdrops =
                unsafe { std::slice::from_raw_parts_mut(backdrop_ptr, self.backdrops.len()) };
            let segments_ptr = segments as *mut LineSegment;
            let segments =
                unsafe { std::slice::from_raw_parts_mut(segments_ptr, self.segments.len()) };
            let draw_record = &self.draw_records[line.path_id as usize];
            let backdrop_record = &self.backdrop_records[line.path_id as usize];
            let backdrop = &mut backdrops[backdrop_record.data_offset as usize
                ..(backdrop_record.data_offset as usize + backdrop_record.data_len as usize)];
            let bbox = draw_record.tile_bbox(self.tiles_size.0, self.tiles_size.1);
            let plan = plan_scan_line(line, bbox);
            let segment_bump = &self.segments_bump[line.path_id as usize];
            if let Some(plan) = plan {
                for y in plan.ymin..plan.ymax {
                    let base = ((y - bbox.y0 as i32) * bbox.tile_stride() as i32) as usize;
                    backdrop[base] += plan.delta;
                }

                let mut last_z = (plan.a * (plan.imin as f32 - 1.0) + plan.b).floor();
                for i in plan.imin..plan.imax {
                    let z = (plan.a * i as f32 + plan.b).floor();
                    let y = (plan.y0 + i as f32 - z) as i32;
                    let x = (plan.x0 + plan.sign * z) as i32;
                    if y < bbox.y0 as i32
                        || y >= bbox.y1 as i32
                        || x < bbox.x0 as i32
                        || x >= bbox.x1 as i32
                    {
                        last_z = z;
                        continue;
                    }
                    let top_edge = if i == plan.imin {
                        (plan.y0 - plan.xy0[1] * TILE_SCALE).abs() <= 1.0e-5
                    } else {
                        last_z == z
                    };
                    if top_edge && x + 1 < bbox.x1 as i32 {
                        let x_bump = (x + 1).max(bbox.x0 as i32);
                        let bump_ix = ((y - bbox.y0 as i32) * bbox.tile_stride() as i32 + x_bump
                            - bbox.x0 as i32) as usize;
                        backdrop[bump_ix] += plan.delta;
                    }

                    let global_ix = (y as u32 * self.tiles_size.0 + x as u32) as usize;
                    let segment = clip_line_to_tile(
                        (plan.xy0, plan.xy1),
                        plan.is_down,
                        plan.is_positive_slope,
                        x,
                        y,
                        i - plan.imin,
                        plan.imax - plan.imin,
                        plan.a,
                        plan.b,
                        global_ix as u32,
                    );
                    let segment_idx = backdrop_record.segment_start
                        + segment_bump.fetch_add(1, Ordering::Relaxed);
                    segments[segment_idx as usize] = segment;
                    last_z = z;
                }
            }
        });

        self.pack_segments_by_tile();
    }

    fn pack_segments_by_tile(&mut self) {
        for backdrop_record in self.backdrop_records.iter() {
            let path_id = backdrop_record.path_id as usize;
            let tile_count = backdrop_record.data_len as usize;
            let ranges = &mut self.tile_segment_ranges[backdrop_record.data_offset as usize
                ..backdrop_record.data_offset as usize + tile_count];
            ranges.fill(TileSegmentRange::default());

            let segment_count = self.segments_bump[path_id].load(Ordering::Relaxed) as usize;
            if tile_count == 0 || segment_count == 0 {
                continue;
            }

            let raw_start = backdrop_record.segment_start as usize;
            let raw_end = raw_start + segment_count;
            let raw_segments = &self.segments[raw_start..raw_end];
            let tiles_width = self.tiles_size.0;
            let counts = &self.segment_tile_counts[backdrop_record.data_offset as usize
                ..backdrop_record.data_offset as usize + tile_count];
            counts.par_iter().for_each(|count| {
                count.store(0, Ordering::Relaxed);
            });

            raw_segments.par_iter().for_each(|segment| {
                let local_ix = Self::local_tile_ix(backdrop_record, segment.tile_id, tiles_width);
                counts[local_ix].fetch_add(1, Ordering::Relaxed);
            });

            Self::fill_ranges_from_counts_parallel(ranges, counts, raw_start as u32);

            let cursors = &mut self.segment_tile_cursors[backdrop_record.data_offset as usize
                ..backdrop_record.data_offset as usize + tile_count];
            for (cursor, range) in cursors.iter().zip(ranges.iter()) {
                cursor.store(range.start, Ordering::Relaxed);
            }
            let packed = &mut self.packed_segments[raw_start..raw_end];
            let packed_ptr = packed.as_mut_ptr() as usize;
            raw_segments.par_iter().for_each(|segment| {
                let mut segment = *segment;
                Self::fill_segment_coverages(&mut segment);
                let local_ix = Self::local_tile_ix(backdrop_record, segment.tile_id, tiles_width);
                let dst = cursors[local_ix].fetch_add(1, Ordering::Relaxed) as usize - raw_start;
                unsafe {
                    (packed_ptr as *mut LineSegment).add(dst).write(segment);
                }
            });

            self.segments[raw_start..raw_end].copy_from_slice(&packed);
        }
    }

    fn local_tile_ix(backdrop_record: &BackdropRecord, tile_id: u32, tiles_width: u32) -> usize {
        let tile_x = tile_id % tiles_width;
        let tile_y = tile_id / tiles_width;
        let local_x = tile_x - backdrop_record.tile_x0;
        let local_y = tile_y - backdrop_record.tile_y0;
        let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
        (local_y * stride + local_x) as usize
    }

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

    fn fill_segment_coverages(segment: &mut LineSegment) {
        let dx = segment.point1.0 - segment.point0.0;
        let dy = segment.point1.1 - segment.point0.1;
        let steps = dx.abs().max(dy.abs()).ceil() as usize;
        if steps == 0 {
            return;
        }

        let x_inc = dx / steps as f32;
        let y_inc = dy / steps as f32;
        let mut x = segment.point0.0;
        let mut y = segment.point0.1;
        let mut row_min = [i16::MAX; TILE_SIZE as usize];
        let mut row_max = [i16::MIN; TILE_SIZE as usize];

        for _ in 0..=steps {
            let px = x.floor() as i32;
            let py = y.floor() as i32;
            if (0..TILE_SIZE as i32).contains(&px) && (0..TILE_SIZE as i32).contains(&py) {
                let row = py as usize;
                row_min[row] = row_min[row].min(px as i16);
                row_max[row] = row_max[row].max(px as i16);
            }
            x += x_inc;
            y += y_inc;
        }

        for py in 0..TILE_SIZE as usize {
            let min_x = row_min[py];
            let max_x = row_max[py];
            if min_x > max_x {
                continue;
            }
            for px in min_x..=max_x {
                let pixel_ix = py * TILE_SIZE as usize + px as usize;
                segment.edges.set(pixel_ix);
            }
        }
    }
}

impl ScanCpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn prepare<'a>(
        &self,
        lines: &'a [Line],
        draw_records: &'a [DrawRecord],
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a mut Vec<i32>,
        tile_segment_ranges: &'a mut Vec<TileSegmentRange>,
        segments: &'a mut Vec<LineSegment>,
        segments_bump: &'a mut Vec<AtomicU32>,
        segment_tile_counts: &'a mut Vec<AtomicU32>,
        segment_tile_cursors: &'a mut Vec<AtomicU32>,
        packed_segments: &'a mut Vec<LineSegment>,
        tiles_size: (u32, u32),
    ) -> ScanCpuPrepared<'a> {
        ScanCpuPrepared {
            lines,
            draw_records,
            backdrop_records,
            tiles_size,
            backdrops,
            tile_segment_ranges,
            segments,
            segments_bump,
            segment_tile_counts,
            segment_tile_cursors,
            packed_segments,
        }
    }
}

pub(crate) struct ScanLinePlan {
    xy0: [f32; 2],
    xy1: [f32; 2],
    is_down: bool,
    is_positive_slope: bool,
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
        is_positive_slope,
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
    is_positive_slope: bool,
    tile_x: i32,
    tile_y: i32,
    seg_within_line: u32,
    seg_count: u32,
    a: f32,
    b: f32,
    tile_id: u32,
) -> LineSegment {
    let (mut xy0, mut xy1) = line;
    let tile_xy = [
        tile_x as f32 * TILE_SIZE as f32,
        tile_y as f32 * TILE_SIZE as f32,
    ];
    let tile_xy1 = [tile_xy[0] + TILE_SIZE as f32, tile_xy[1] + TILE_SIZE as f32];

    if seg_within_line > 0 {
        let z_prev = (a * (seg_within_line as f32 - 1.0) + b).floor();
        let z = (a * seg_within_line as f32 + b).floor();
        if z == z_prev {
            let mut xt = xy0[0] + (xy1[0] - xy0[0]) * (tile_xy[1] - xy0[1]) / (xy1[1] - xy0[1]);
            xt = xt.clamp(tile_xy[0] + 1e-3, tile_xy1[0]);
            xy0 = [xt, tile_xy[1]];
        } else {
            let x_clip = if is_positive_slope {
                tile_xy[0]
            } else {
                tile_xy1[0]
            };
            let mut yt = xy0[1] + (xy1[1] - xy0[1]) * (x_clip - xy0[0]) / (xy1[0] - xy0[0]);
            yt = yt.clamp(tile_xy[1] + 1e-3, tile_xy1[1]);
            xy0 = [x_clip, yt];
        }
    }
    if seg_within_line < seg_count.saturating_sub(1) {
        let z_next = (a * (seg_within_line as f32 + 1.0) + b).floor();
        let z = (a * seg_within_line as f32 + b).floor();
        if z == z_next {
            let mut xt = xy0[0] + (xy1[0] - xy0[0]) * (tile_xy1[1] - xy0[1]) / (xy1[1] - xy0[1]);
            xt = xt.clamp(tile_xy[0] + 1e-3, tile_xy1[0]);
            xy1 = [xt, tile_xy1[1]];
        } else {
            let x_clip = if is_positive_slope {
                tile_xy1[0]
            } else {
                tile_xy[0]
            };
            let mut yt = xy0[1] + (xy1[1] - xy0[1]) * (x_clip - xy0[0]) / (xy1[0] - xy0[0]);
            yt = yt.clamp(tile_xy[1] + 1e-3, tile_xy1[1]);
            xy1 = [x_clip, yt];
        }
    }

    let mut y_edge = 1e9f32;
    let mut p0 = (xy0[0] - tile_xy[0], xy0[1] - tile_xy[1]);
    let mut p1 = (xy1[0] - tile_xy[0], xy1[1] - tile_xy[1]);
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
        tile_id,
        edges: TileMask::new(),
    }
}

fn span(a: f32, b: f32) -> u32 {
    let hi = a.max(b).ceil();
    let lo = a.min(b).floor();
    (hi - lo).max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use peniko::Color;

    use std::sync::atomic::AtomicU32;

    use super::{ScanCpuPipeline, ScanCpuPrepared, plan_scan_line};
    use crate::shared::{
        bd_record::BackdropRecord,
        bounds::{PixelBounds, TileBbox},
        brush::Brush,
        draw_record::DrawRecord,
        draw_record::DrawTag,
        fill::FillRule,
        line::Line,
        line_seg::LineSegment,
        path::PathRecord,
        tile_seg_range::TileSegmentRange,
    };

    fn one_tile_draw_record() -> DrawRecord {
        DrawRecord {
            path_id: Some(0),
            tag: DrawTag::Brush,
            brush: Brush::Solid(Color::BLACK),
            fill_rule: FillRule::NonZero,
            pixel_bounds: PixelBounds {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
            solid_rect: false,
            allow_solid_override: true,
        }
    }

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
        let draw_records = [one_tile_draw_record()];
        let backdrop_records = [one_tile_backdrop_record(1)];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 1];
        let mut segments = vec![LineSegment::default(); 1];
        let mut segments_bump = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_counts = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_cursors = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut packed_segments = vec![LineSegment::default(); 1];

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &draw_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                &mut packed_segments,
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
        let draw_records = [one_tile_draw_record()];
        let backdrop_records = [one_tile_backdrop_record(1)];
        let mut backdrops = vec![0];
        let mut tile_segment_ranges = vec![TileSegmentRange::default(); 1];
        let mut segments = vec![LineSegment::default(); 1];
        let mut segments_bump = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_counts = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_cursors = vec![0]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut packed_segments = vec![LineSegment::default(); 1];

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &draw_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                &mut packed_segments,
                (1, 1),
            )
            .run();

        assert_eq!(backdrops, vec![0]);
        assert_eq!(segments_bump[0].load(Ordering::Relaxed), 1);
        assert_eq!(
            tile_segment_ranges[0],
            TileSegmentRange { start: 0, end: 1 }
        );
        assert_eq!(segments[0].tile_id, 0);
        assert!((segments[0].point0.0 - 4.0).abs() < 1e-3);
        assert!((segments[0].point1.0 - 4.0).abs() < 1e-3);
        assert!((segments[0].point0.1 - 0.0).abs() < 1e-6);
        assert!((segments[0].point1.1 - 16.0).abs() < 1e-6);
        assert!(segments[0].edges.any());
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
        let draw_records = [DrawRecord {
            path_id: Some(0),
            tag: DrawTag::Brush,
            brush: Brush::Solid(Color::BLACK),
            fill_rule: FillRule::NonZero,
            pixel_bounds: PixelBounds {
                x0: 0,
                y0: 0,
                x1: 32,
                y1: 16,
            },
            solid_rect: false,
            allow_solid_override: true,
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
        let mut segment_tile_counts = vec![0; 2]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut segment_tile_cursors = vec![0; 2]
            .into_iter()
            .map(std::sync::atomic::AtomicU32::new)
            .collect();
        let mut packed_segments = vec![LineSegment::default(); 4];

        ScanCpuPipeline::new()
            .prepare(
                &lines,
                &draw_records,
                &backdrop_records,
                &mut backdrops,
                &mut tile_segment_ranges,
                &mut segments,
                &mut segments_bump,
                &mut segment_tile_counts,
                &mut segment_tile_cursors,
                &mut packed_segments,
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
        assert_eq!(segments[0].tile_id, 0);
        assert_eq!(segments[1].tile_id, 0);
        assert_eq!(segments[2].tile_id, 1);
        assert_eq!(segments[3].tile_id, 1);
        assert!(segments.iter().all(|segment| segment.edges.any()));
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
