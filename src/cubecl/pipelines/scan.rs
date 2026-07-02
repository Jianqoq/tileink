use ::cubecl::prelude::*;

use crate::cubecl::{
    profile::profile_launch,
    renderer::{ScanBuffers, SceneBuffers},
    types::{CubeBufferLengths, SCAN_CHUNK_SIZE},
};

const WORKGROUP_SIZE: u32 = 256;
const DDA_TOP_EDGE_EPSILON: f32 = 1.0e-5;
const TILE_BOUNDARY_EPSILON: f32 = 1.0e-4;

pub(crate) struct ScanPipeline;

impl ScanPipeline {
    pub(crate) fn run<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &mut ScanBuffers,
        lengths: CubeBufferLengths,
    ) {
        let backdrop_len = lengths.backdrop_len as u32;
        let path_count = lengths.path_count as u32;
        let line_count = lengths.line_count as u32;
        let scan_chunk_count = lengths.scan_chunk_count as u32;
        let segment_capacity = lengths.segment_capacity as u32;
        let clear_len = backdrop_len.max(path_count).max(scan_chunk_count);

        if clear_len > 0 {
            profile_launch(client, "scan_clear", || {
                scan_clear::launch::<R>(
                    client,
                    cube_count(clear_len),
                    CubeDim::new_1d(WORKGROUP_SIZE),
                    clear_len,
                    backdrop_len,
                    path_count,
                    scan_chunk_count,
                    unsafe { scan.backdrops.arg() },
                    unsafe { scan.tile_segment_range_starts.arg() },
                    unsafe { scan.tile_segment_range_ends.arg() },
                    unsafe { scan.segment_tile_counts.arg() },
                    unsafe { scan.segment_tile_cursors.arg() },
                    unsafe { scan.segment_bumps.arg() },
                    unsafe { scan.chunk_totals.arg() },
                    unsafe { scan.chunk_offsets.arg() },
                );
            });
        }

        if line_count > 0 {
            profile_launch(client, "scan_count", || {
                scan_count::launch::<R>(
                    client,
                    cube_count(line_count),
                    CubeDim::new_1d(WORKGROUP_SIZE),
                    line_count,
                    unsafe { scene.line_path_ids.arg() },
                    unsafe { scene.line_p0x.arg() },
                    unsafe { scene.line_p0y.arg() },
                    unsafe { scene.line_p1x.arg() },
                    unsafe { scene.line_p1y.arg() },
                    unsafe { scene.path_flags.arg() },
                    unsafe { scene.backdrop_data_offsets.arg() },
                    unsafe { scene.backdrop_tile_x0.arg() },
                    unsafe { scene.backdrop_tile_y0.arg() },
                    unsafe { scene.backdrop_tile_x1.arg() },
                    unsafe { scene.backdrop_tile_y1.arg() },
                    unsafe { scan.backdrops.arg() },
                    unsafe { scan.segment_tile_counts.arg() },
                );
            });
        }

        if scan_chunk_count > 0 {
            profile_launch(client, "scan_prefix_chunks", || {
                scan_prefix_chunks::launch::<R>(
                    client,
                    CubeCount::Static(scan_chunk_count, 1, 1),
                    CubeDim::new_1d(SCAN_CHUNK_SIZE),
                    SCAN_CHUNK_SIZE as usize,
                    unsafe { scene.scan_chunk_backdrop_offsets.arg() },
                    unsafe { scene.scan_chunk_lens.arg() },
                    unsafe { scan.segment_tile_counts.arg() },
                    unsafe { scan.tile_segment_range_starts.arg() },
                    unsafe { scan.tile_segment_range_ends.arg() },
                    unsafe { scan.chunk_totals.arg() },
                );
            });
        }

        if path_count > 0 {
            profile_launch(client, "scan_chunk_offsets", || {
                scan_chunk_offsets::launch::<R>(
                    client,
                    cube_count(path_count),
                    CubeDim::new_1d(WORKGROUP_SIZE),
                    path_count,
                    unsafe { scene.backdrop_segment_starts.arg() },
                    unsafe { scene.scan_chunk_range_starts.arg() },
                    unsafe { scene.scan_chunk_range_ends.arg() },
                    unsafe { scan.chunk_totals.arg() },
                    unsafe { scan.chunk_offsets.arg() },
                    unsafe { scan.segment_bumps.arg() },
                );
            });
        }

        if scan_chunk_count > 0 {
            profile_launch(client, "scan_apply_chunk_offsets", || {
                scan_apply_chunk_offsets::launch::<R>(
                    client,
                    CubeCount::Static(scan_chunk_count, 1, 1),
                    CubeDim::new_1d(SCAN_CHUNK_SIZE),
                    unsafe { scene.scan_chunk_backdrop_offsets.arg() },
                    unsafe { scene.scan_chunk_lens.arg() },
                    unsafe { scan.chunk_offsets.arg() },
                    unsafe { scan.tile_segment_range_starts.arg() },
                    unsafe { scan.tile_segment_range_ends.arg() },
                    unsafe { scan.segment_tile_cursors.arg() },
                );
            });
        }

        if line_count > 0 && segment_capacity > 0 {
            profile_launch(client, "scan_emit", || {
                scan_emit::launch::<R>(
                    client,
                    cube_count(line_count),
                    CubeDim::new_1d(WORKGROUP_SIZE),
                    line_count,
                    segment_capacity,
                    unsafe { scene.line_path_ids.arg() },
                    unsafe { scene.line_p0x.arg() },
                    unsafe { scene.line_p0y.arg() },
                    unsafe { scene.line_p1x.arg() },
                    unsafe { scene.line_p1y.arg() },
                    unsafe { scene.path_flags.arg() },
                    unsafe { scene.backdrop_data_offsets.arg() },
                    unsafe { scene.backdrop_tile_x0.arg() },
                    unsafe { scene.backdrop_tile_y0.arg() },
                    unsafe { scene.backdrop_tile_x1.arg() },
                    unsafe { scene.backdrop_tile_y1.arg() },
                    unsafe { scan.segment_tile_cursors.arg() },
                    unsafe { scan.segment_p0x.arg() },
                    unsafe { scan.segment_p0y.arg() },
                    unsafe { scan.segment_p1x.arg() },
                    unsafe { scan.segment_p1y.arg() },
                    unsafe { scan.segment_y_edge.arg() },
                );
            });
        }
    }
}

fn cube_count(items: u32) -> CubeCount {
    CubeCount::Static(items.div_ceil(WORKGROUP_SIZE), 1, 1)
}

#[cube(launch)]
fn scan_clear(
    len: u32,
    backdrop_len: u32,
    path_count: u32,
    scan_chunk_count: u32,
    backdrops: &mut Array<Atomic<i32>>,
    range_starts: &mut Array<u32>,
    range_ends: &mut Array<u32>,
    segment_tile_counts: &mut Array<Atomic<u32>>,
    segment_tile_cursors: &mut Array<Atomic<u32>>,
    segment_bumps: &mut Array<u32>,
    chunk_totals: &mut Array<u32>,
    chunk_offsets: &mut Array<u32>,
) {
    let ix = ABSOLUTE_POS as u32;
    if ix >= len {
        terminate!();
    }
    let i = ix as usize;
    if ix < backdrop_len {
        backdrops[i].store(0);
        range_starts[i] = 0;
        range_ends[i] = 0;
        segment_tile_counts[i].store(0);
        segment_tile_cursors[i].store(0);
    }
    if ix < path_count {
        segment_bumps[i] = 0;
    }
    if ix < scan_chunk_count {
        chunk_totals[i] = 0;
        chunk_offsets[i] = 0;
    }
}

#[cube(launch)]
fn scan_count(
    line_count: u32,
    line_path_ids: &Array<u32>,
    line_p0x: &Array<f32>,
    line_p0y: &Array<f32>,
    line_p1x: &Array<f32>,
    line_p1y: &Array<f32>,
    path_flags: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &mut Array<Atomic<i32>>,
    segment_tile_counts: &mut Array<Atomic<u32>>,
) {
    let line_ix = ABSOLUTE_POS as u32;
    if line_ix >= line_count {
        terminate!();
    }
    let line_i = line_ix as usize;
    let path_id = line_path_ids[line_i];
    let path_i = path_id as usize;
    if path_i >= backdrop_data_offsets.len() {
        terminate!();
    }

    let bbox_x0 = backdrop_tile_x0[path_i];
    let bbox_y0 = backdrop_tile_y0[path_i];
    let bbox_x1 = backdrop_tile_x1[path_i];
    let bbox_y1 = backdrop_tile_y1[path_i];
    let bbox_stride = bbox_x1 - bbox_x0;
    if bbox_stride == 0 || bbox_y0 >= bbox_y1 {
        terminate!();
    }

    let keep_horizontal_tile_edges = path_flags[path_i] >= 1;
    let p0x = line_p0x[line_i];
    let p0y = line_p0y[line_i];
    let p1x = line_p1x[line_i];
    let p1y = line_p1y[line_i];
    let is_down = p1y >= p0y;
    let mut xy0x = p0x;
    let mut xy0y = p0y;
    let mut xy1x = p1x;
    let mut xy1y = p1y;
    if !is_down {
        xy0x = p1x;
        xy0y = p1y;
        xy1x = p0x;
        xy1y = p0y;
    }

    let tile_scale = f32::new(0.0625_f32);
    let s0x = xy0x * tile_scale;
    let s0y = xy0y * tile_scale;
    let s1x = xy1x * tile_scale;
    let s1y = xy1y * tile_scale;
    let count_x = span(s0x, s1x) - 1;
    let count = count_x + span(s0y, s1y);
    let dx = (s1x - s0x).abs();
    let dy = s1y - s0y;
    if dx + dy == 0.0 || (dy == 0.0 && s0y.floor() == s0y && !keep_horizontal_tile_edges) {
        terminate!();
    }
    // Stroke outlines keep horizontal boundary edges as fine segments; those
    // edges must not also emit coarse top-edge carry. Fill paths keep the
    // historical top-edge carry because their boundary horizontals are owned
    // by the coarse scan rule instead of fine segments.
    let line_needs_top_edge_carry = !keep_horizontal_tile_edges
        || (s0y != s1y && (s0y.floor() < s1y.floor() || bbox_y1 > bbox_y0 + 1));
    let skip_initial_top_edge_carry = keep_horizontal_tile_edges && bbox_y0 == 0 && xy1x < xy0x;

    let idxdy = 1.0 / (dx + dy);
    let mut a = dx * idxdy;
    let is_positive_slope = s1x >= s0x;
    // CubeCL 0.10 can mis-lower `let x = if ...` in WGSL; keep scalar selections explicit.
    let mut sign = f32::new(-1.0_f32);
    if is_positive_slope {
        sign = f32::new(1.0_f32);
    }
    let xt0 = (s0x * sign).floor();
    let c = s0x * sign - xt0;
    let y0 = s0y.floor();
    let mut ytop = y0 + 1.0;
    if s0y == s1y {
        ytop = s0y.ceil();
    }
    let b = ((dy * c + dx * (ytop - s0y)) * idxdy).min(0.99999994);
    let robust_err = (a * (count as f32 - 1.0) + b).floor() - count_x as f32;
    if robust_err != 0.0 {
        if robust_err > 0.0 {
            a -= f32::new(0.0000002_f32);
        } else {
            a += f32::new(0.0000002_f32);
        }
    }
    let mut x0 = xt0 * sign - f32::new(1.0_f32);
    if is_positive_slope {
        x0 = xt0 * sign;
    }
    let xmin = s0x.min(s1x);
    if s0y >= bbox_y1 as f32 || s1y < bbox_y0 as f32 || xmin >= bbox_x1 as f32 {
        terminate!();
    }

    let mut imin = 0u32;
    if s0y < bbox_y0 as f32 {
        let mut iminf = ((bbox_y0 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + iminf - (a * iminf + b).floor() < bbox_y0 as f32 {
            iminf += 1.0;
        }
        imin = iminf as u32;
    }
    let mut imax = count;
    if s1y > bbox_y1 as f32 {
        let mut imaxf = ((bbox_y1 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + imaxf - (a * imaxf + b).floor() < bbox_y1 as f32 {
            imaxf += 1.0;
        }
        imax = imaxf as u32;
    }

    let mut delta = i32::new(1);
    if is_down {
        delta = i32::new(-1);
    }
    let mut ymin = 0i32;
    let mut ymax = 0i32;
    if s0x.max(s1x) <= bbox_x0 as f32 {
        ymin = s0y.ceil() as i32;
        ymax = s1y.ceil() as i32;
        imax = imin;
    } else {
        let mut fudge = f32::new(1.0_f32);
        if is_positive_slope {
            fudge = f32::new(0.0_f32);
        }
        if xmin < bbox_x0 as f32 {
            let mut f = ((sign * (bbox_x0 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox_x0 as f32) == is_positive_slope {
                f += 1.0;
            }
            let ynext = (y0 + f - (a * f + b).floor() + 1.0) as i32;
            if is_positive_slope {
                if f as u32 > imin {
                    let mut ystart = y0 + f32::new(1.0_f32);
                    if y0 == s0y {
                        ystart = y0;
                    }
                    ymin = ystart as i32;
                    ymax = ynext;
                    imin = f as u32;
                }
            } else if (f as u32) < imax {
                ymin = ynext;
                ymax = s1y.ceil() as i32;
                imax = f as u32;
            }
        }
        if s0x.max(s1x) > bbox_x1 as f32 {
            let mut f = ((sign * (bbox_x1 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox_x1 as f32) == is_positive_slope {
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
    ymin = ymin.max(bbox_y0 as i32);
    ymax = ymax.min(bbox_y1 as i32);
    let tile_boundary_epsilon = f32::new(TILE_BOUNDARY_EPSILON);
    if ymin == bbox_y0 as i32 && ymax > ymin && s0y < bbox_y0 as f32 && s1y > bbox_y0 as f32 {
        let dx_left = s1x - s0x;
        if dx_left != 0.0 {
            let left_x = bbox_x0 as f32;
            let top_y = bbox_y0 as f32;
            let top_x = s0x + (s1x - s0x) * ((top_y - s0y) / (s1y - s0y));
            let left_y = s0y + (s1y - s0y) * ((left_x - s0x) / dx_left);
            if top_x - left_x >= -tile_boundary_epsilon
                && top_x - left_x <= tile_boundary_epsilon
                && left_y - top_y >= -tile_boundary_epsilon
                && left_y - top_y <= tile_boundary_epsilon
            {
                ymin += 1;
            }
        }
    }

    let data_offset = backdrop_data_offsets[path_i];
    let mut y = ymin;
    while y < ymax {
        let local = ((y - bbox_y0 as i32) * bbox_stride as i32) as u32;
        backdrops[(data_offset + local) as usize].fetch_add(delta);
        y += 1;
    }
    if imin < imax
        && s0y < bbox_y0 as f32 - tile_boundary_epsilon
        && s1y > bbox_y0 as f32 + tile_boundary_epsilon
    {
        let top_y = bbox_y0 as f32;
        let top_x = s0x + (s1x - s0x) * ((top_y - s0y) / (s1y - s0y));
        if top_x >= bbox_x0 as f32 - tile_boundary_epsilon && top_x < bbox_x1 as f32 {
            // Top-clipped crossings have no original DDA top-edge event. The
            // clipped boundary contributes from the first tile whose left edge
            // is at or to the right of the crossing; exact tile-boundary
            // crossings stay on that boundary instead of advancing one tile.
            let mut x_bump = (top_x - tile_boundary_epsilon).ceil() as i32;
            if top_x - bbox_x0 as f32 <= tile_boundary_epsilon {
                x_bump = bbox_x0 as i32 + 1;
            }
            if x_bump >= bbox_x0 as i32 && x_bump < bbox_x1 as i32 {
                let bump_local = (x_bump - bbox_x0 as i32) as u32;
                backdrops[(data_offset + bump_local) as usize].fetch_add(delta);
            }
        }
    }

    let mut last_z = (a * (imin as f32 - 1.0) + b).floor();
    let mut i = imin;
    while i < imax {
        let z = (a * i as f32 + b).floor();
        let tile_y = (y0 + i as f32 - z) as i32;
        let tile_x = (x0 + sign * z) as i32;
        if tile_y >= bbox_y0 as i32
            && tile_y < bbox_y1 as i32
            && tile_x >= bbox_x0 as i32
            && tile_x < bbox_x1 as i32
        {
            let mut top_edge = last_z == z;
            let mut initial_top_edge = false;
            if i == imin {
                initial_top_edge =
                    imin == 0 && (y0 - xy0y * tile_scale).abs() <= f32::new(DDA_TOP_EDGE_EPSILON);
                top_edge = initial_top_edge;
            }
            if line_needs_top_edge_carry
                && top_edge
                && !(initial_top_edge && skip_initial_top_edge_carry)
                && tile_x + 1 < bbox_x1 as i32
            {
                let x_bump = (tile_x + 1).max(bbox_x0 as i32);
                let bump_local = ((tile_y - bbox_y0 as i32) * bbox_stride as i32 + x_bump
                    - bbox_x0 as i32) as u32;
                backdrops[(data_offset + bump_local) as usize].fetch_add(delta);
            }

            let local_ix = local_tile_ix(tile_x, tile_y, bbox_x0, bbox_y0, bbox_x1);
            segment_tile_counts[(data_offset + local_ix) as usize].fetch_add(1);
        }
        last_z = z;
        i += 1;
    }
}

#[cube(launch)]
fn scan_prefix_chunks(
    #[comptime] chunk_size: usize,
    scan_chunk_backdrop_offsets: &Array<u32>,
    scan_chunk_lens: &Array<u32>,
    segment_tile_counts: &Array<Atomic<u32>>,
    range_starts: &mut Array<u32>,
    range_ends: &mut Array<u32>,
    chunk_totals: &mut Array<u32>,
) {
    let chunk_ix = CUBE_POS;
    let lane = UNIT_POS as usize;
    let chunk_offset = scan_chunk_backdrop_offsets[chunk_ix];
    let chunk_len = scan_chunk_lens[chunk_ix];
    let mut shared = SharedMemory::<u32>::new(chunk_size);

    let mut count = u32::new(0);
    if lane < chunk_len as usize {
        count = segment_tile_counts[(chunk_offset + lane as u32) as usize].load();
    }
    shared[lane] = count;
    sync_cube();

    let mut step = 1usize;
    while step < chunk_size {
        let mut add = u32::new(0);
        if lane >= step {
            add = shared[lane - step];
        }
        sync_cube();
        if lane >= step {
            shared[lane] += add;
        }
        sync_cube();
        step *= 2;
    }

    if lane < chunk_len as usize {
        let inclusive = shared[lane];
        let exclusive = inclusive - count;
        let ix = (chunk_offset + lane as u32) as usize;
        range_starts[ix] = exclusive;
        range_ends[ix] = inclusive;
    }
    if lane + 1 == chunk_len as usize {
        chunk_totals[chunk_ix] = shared[lane];
    }
}

#[cube(launch)]
fn scan_chunk_offsets(
    path_count: u32,
    backdrop_segment_starts: &Array<u32>,
    scan_chunk_range_starts: &Array<u32>,
    scan_chunk_range_ends: &Array<u32>,
    chunk_totals: &Array<u32>,
    chunk_offsets: &mut Array<u32>,
    segment_bumps: &mut Array<u32>,
) {
    let path_id = ABSOLUTE_POS as u32;
    if path_id >= path_count {
        terminate!();
    }
    let path_i = path_id as usize;
    let mut next = backdrop_segment_starts[path_i];
    let mut chunk_ix = scan_chunk_range_starts[path_i];
    let chunk_end = scan_chunk_range_ends[path_i];
    while chunk_ix < chunk_end {
        let ix = chunk_ix as usize;
        chunk_offsets[ix] = next;
        next += chunk_totals[ix];
        chunk_ix += 1;
    }
    segment_bumps[path_i] = next - backdrop_segment_starts[path_i];
}

#[cube(launch)]
fn scan_apply_chunk_offsets(
    scan_chunk_backdrop_offsets: &Array<u32>,
    scan_chunk_lens: &Array<u32>,
    chunk_offsets: &Array<u32>,
    range_starts: &mut Array<u32>,
    range_ends: &mut Array<u32>,
    segment_tile_cursors: &mut Array<Atomic<u32>>,
) {
    let chunk_ix = CUBE_POS;
    let lane = UNIT_POS;
    let chunk_len = scan_chunk_lens[chunk_ix];
    if lane >= chunk_len {
        terminate!();
    }
    let ix = (scan_chunk_backdrop_offsets[chunk_ix] + lane) as usize;
    let base = chunk_offsets[chunk_ix];
    let start = range_starts[ix] + base;
    let end = range_ends[ix] + base;
    range_starts[ix] = start;
    range_ends[ix] = end;
    segment_tile_cursors[ix].store(start);
}

#[cube(launch)]
fn scan_emit(
    line_count: u32,
    segment_capacity: u32,
    line_path_ids: &Array<u32>,
    line_p0x: &Array<f32>,
    line_p0y: &Array<f32>,
    line_p1x: &Array<f32>,
    line_p1y: &Array<f32>,
    path_flags: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    segment_tile_cursors: &mut Array<Atomic<u32>>,
    segment_p0x: &mut Array<f32>,
    segment_p0y: &mut Array<f32>,
    segment_p1x: &mut Array<f32>,
    segment_p1y: &mut Array<f32>,
    segment_y_edge: &mut Array<f32>,
) {
    let line_ix = ABSOLUTE_POS as u32;
    if line_ix >= line_count {
        terminate!();
    }
    let line_i = line_ix as usize;
    let path_id = line_path_ids[line_i];
    let path_i = path_id as usize;
    if path_i >= backdrop_data_offsets.len() {
        terminate!();
    }

    let bbox_x0 = backdrop_tile_x0[path_i];
    let bbox_y0 = backdrop_tile_y0[path_i];
    let bbox_x1 = backdrop_tile_x1[path_i];
    let bbox_y1 = backdrop_tile_y1[path_i];
    let bbox_stride = bbox_x1 - bbox_x0;
    if bbox_stride == 0 || bbox_y0 >= bbox_y1 {
        terminate!();
    }

    let keep_horizontal_tile_edges = path_flags[path_i] >= 1;
    let p0x = line_p0x[line_i];
    let p0y = line_p0y[line_i];
    let p1x = line_p1x[line_i];
    let p1y = line_p1y[line_i];
    let is_down = p1y >= p0y;
    let mut xy0x = p0x;
    let mut xy0y = p0y;
    let mut xy1x = p1x;
    let mut xy1y = p1y;
    if !is_down {
        xy0x = p1x;
        xy0y = p1y;
        xy1x = p0x;
        xy1y = p0y;
    }

    let tile_scale = f32::new(0.0625_f32);
    let s0x = xy0x * tile_scale;
    let s0y = xy0y * tile_scale;
    let s1x = xy1x * tile_scale;
    let s1y = xy1y * tile_scale;
    let count_x = span(s0x, s1x) - 1;
    let count = count_x + span(s0y, s1y);
    let dx = (s1x - s0x).abs();
    let dy = s1y - s0y;
    if dx + dy == 0.0 || (dy == 0.0 && s0y.floor() == s0y && !keep_horizontal_tile_edges) {
        terminate!();
    }

    let idxdy = 1.0 / (dx + dy);
    let mut a = dx * idxdy;
    let is_positive_slope = s1x >= s0x;
    // Keep this as statements for the same CubeCL lowering reason documented in scan_count.
    let mut sign = f32::new(-1.0_f32);
    if is_positive_slope {
        sign = f32::new(1.0_f32);
    }
    let xt0 = (s0x * sign).floor();
    let c = s0x * sign - xt0;
    let y0 = s0y.floor();
    let mut ytop = y0 + 1.0;
    if s0y == s1y {
        ytop = s0y.ceil();
    }
    let b = ((dy * c + dx * (ytop - s0y)) * idxdy).min(0.99999994);
    let robust_err = (a * (count as f32 - 1.0) + b).floor() - count_x as f32;
    if robust_err != 0.0 {
        if robust_err > 0.0 {
            a -= f32::new(0.0000002_f32);
        } else {
            a += f32::new(0.0000002_f32);
        }
    }
    let mut x0 = xt0 * sign - f32::new(1.0_f32);
    if is_positive_slope {
        x0 = xt0 * sign;
    }
    let xmin = s0x.min(s1x);
    if s0y >= bbox_y1 as f32 || s1y < bbox_y0 as f32 || xmin >= bbox_x1 as f32 {
        terminate!();
    }

    let mut imin = 0u32;
    if s0y < bbox_y0 as f32 {
        let mut iminf = ((bbox_y0 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + iminf - (a * iminf + b).floor() < bbox_y0 as f32 {
            iminf += 1.0;
        }
        imin = iminf as u32;
    }
    let mut imax = count;
    if s1y > bbox_y1 as f32 {
        let mut imaxf = ((bbox_y1 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + imaxf - (a * imaxf + b).floor() < bbox_y1 as f32 {
            imaxf += 1.0;
        }
        imax = imaxf as u32;
    }

    if s0x.max(s1x) <= bbox_x0 as f32 {
        imax = imin;
    } else {
        let mut fudge = f32::new(1.0_f32);
        if is_positive_slope {
            fudge = f32::new(0.0_f32);
        }
        if xmin < bbox_x0 as f32 {
            let mut f = ((sign * (bbox_x0 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox_x0 as f32) == is_positive_slope {
                f += 1.0;
            }
            if is_positive_slope {
                if f as u32 > imin {
                    imin = f as u32;
                }
            } else if (f as u32) < imax {
                imax = f as u32;
            }
        }
        if s0x.max(s1x) > bbox_x1 as f32 {
            let mut f = ((sign * (bbox_x1 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox_x1 as f32) == is_positive_slope {
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

    let data_offset = backdrop_data_offsets[path_i];
    let mut i = imin;
    while i < imax {
        let z = (a * i as f32 + b).floor();
        let tile_y = (y0 + i as f32 - z) as i32;
        let tile_x = (x0 + sign * z) as i32;
        if tile_y >= bbox_y0 as i32
            && tile_y < bbox_y1 as i32
            && tile_x >= bbox_x0 as i32
            && tile_x < bbox_x1 as i32
        {
            let local_ix = local_tile_ix(tile_x, tile_y, bbox_x0, bbox_y0, bbox_x1);
            let dst = segment_tile_cursors[(data_offset + local_ix) as usize].fetch_add(1);
            if dst < segment_capacity {
                write_clipped_segment(
                    dst as usize,
                    xy0x,
                    xy0y,
                    xy1x,
                    xy1y,
                    is_down,
                    keep_horizontal_tile_edges,
                    tile_x,
                    tile_y,
                    segment_p0x,
                    segment_p0y,
                    segment_p1x,
                    segment_p1y,
                    segment_y_edge,
                );
            }
        }
        i += 1;
    }
}

#[cube]
fn span(a: f32, b: f32) -> u32 {
    let mut hi = a.ceil();
    if b > a {
        hi = b.ceil();
    }
    let mut lo = a.floor();
    if b < a {
        lo = b.floor();
    }
    let mut value = hi - lo;
    if value < 1.0 {
        value = 1.0;
    }
    value as u32
}

#[cube]
fn local_tile_ix(tile_x: i32, tile_y: i32, bbox_x0: u32, bbox_y0: u32, bbox_x1: u32) -> u32 {
    let local_x = tile_x as u32 - bbox_x0;
    let local_y = tile_y as u32 - bbox_y0;
    local_y * (bbox_x1 - bbox_x0) + local_x
}

#[cube]
fn write_clipped_segment(
    dst: usize,
    line_x0: f32,
    line_y0: f32,
    line_x1: f32,
    line_y1: f32,
    is_down: bool,
    keep_horizontal_tile_edges: bool,
    tile_x: i32,
    tile_y: i32,
    segment_p0x: &mut Array<f32>,
    segment_p0y: &mut Array<f32>,
    segment_p1x: &mut Array<f32>,
    segment_p1y: &mut Array<f32>,
    segment_y_edge: &mut Array<f32>,
) {
    let tile_size = f32::new(16.0_f32);
    let tile_min_x = tile_x as f32 * tile_size;
    let tile_min_y = tile_y as f32 * tile_size;
    let tile_max_x = tile_min_x + tile_size;
    let tile_max_y = tile_min_y + tile_size;

    let dx = line_x1 - line_x0;
    let dy = line_y1 - line_y0;
    let mut t0 = f32::new(0.0_f32);
    let mut t1 = f32::new(1.0_f32);
    let mut valid = true;

    let mut p = -dx;
    let mut q = line_x0 - tile_min_x;
    if p == 0.0 {
        if q < 0.0 {
            valid = false;
        }
    } else {
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                valid = false;
            } else if r > t0 {
                t0 = r;
            }
        } else if r < t0 {
            valid = false;
        } else if r < t1 {
            t1 = r;
        }
    }

    p = dx;
    q = tile_max_x - line_x0;
    if p == 0.0 {
        if q < 0.0 {
            valid = false;
        }
    } else {
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                valid = false;
            } else if r > t0 {
                t0 = r;
            }
        } else if r < t0 {
            valid = false;
        } else if r < t1 {
            t1 = r;
        }
    }

    p = -dy;
    q = line_y0 - tile_min_y;
    if p == 0.0 {
        if q < 0.0 {
            valid = false;
        }
    } else {
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                valid = false;
            } else if r > t0 {
                t0 = r;
            }
        } else if r < t0 {
            valid = false;
        } else if r < t1 {
            t1 = r;
        }
    }

    p = dy;
    q = tile_max_y - line_y0;
    if p == 0.0 {
        if q < 0.0 {
            valid = false;
        }
    } else {
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                valid = false;
            } else if r > t0 {
                t0 = r;
            }
        } else if r < t0 {
            valid = false;
        } else if r < t1 {
            t1 = r;
        }
    }

    let mut xy0x = line_x0.clamp(tile_min_x, tile_max_x);
    let mut xy0y = line_y0.clamp(tile_min_y, tile_max_y);
    let mut xy1x = line_x1.clamp(tile_min_x, tile_max_x);
    let mut xy1y = line_y1.clamp(tile_min_y, tile_max_y);
    if valid {
        xy0x = line_x0 + dx * t0;
        xy0y = line_y0 + dy * t0;
        xy1x = line_x0 + dx * t1;
        xy1y = line_y0 + dy * t1;
    }

    let mut y_edge = f32::new(1000000000.0_f32);
    let mut p0x = (xy0x - tile_min_x).clamp(0.0, tile_size);
    let mut p0y = (xy0y - tile_min_y).clamp(0.0, tile_size);
    let mut p1x = (xy1x - tile_min_x).clamp(0.0, tile_size);
    let mut p1y = (xy1y - tile_min_y).clamp(0.0, tile_size);
    let boundary_epsilon = f32::new(TILE_BOUNDARY_EPSILON);
    if p0x <= boundary_epsilon {
        p0x = 0.0;
    } else if tile_size - p0x <= boundary_epsilon {
        p0x = tile_size;
    }
    if p0y <= boundary_epsilon {
        p0y = 0.0;
    } else if tile_size - p0y <= boundary_epsilon {
        p0y = tile_size;
    }
    if p1x <= boundary_epsilon {
        p1x = 0.0;
    } else if tile_size - p1x <= boundary_epsilon {
        p1x = tile_size;
    }
    if p1y <= boundary_epsilon {
        p1y = 0.0;
    } else if tile_size - p1y <= boundary_epsilon {
        p1y = tile_size;
    }
    let epsilon = f32::new(0.000001_f32);

    if p0x == 0.0 {
        if p1x == 0.0 {
            p0x = epsilon;
            if p0y == 0.0 {
                p1x = epsilon;
                p1y = tile_size;
            } else {
                p1x = f32::new(2.0_f32) * epsilon;
                p1y = p0y;
            }
        } else if p0y == 0.0 {
            // Diagonal edges passing exactly through top-left are owned by the top edge.
            // Stroke outlines keep horizontal top edges on tile boundaries so their paired
            // bottom edges cannot fill every row below the stroke.
            if (keep_horizontal_tile_edges && p1y == 0.0)
                || (p1x <= 1.0 + boundary_epsilon && p1y <= 1.0 + boundary_epsilon)
            {
                y_edge = p0y;
            }
            p0x = epsilon;
        } else {
            y_edge = p0y;
        }
    } else if p1x == 0.0 {
        if p1y == 0.0 {
            if keep_horizontal_tile_edges && p0y == 0.0 {
                y_edge = p1y;
            }
            p1x = epsilon;
        } else {
            y_edge = p1y;
        }
    }
    if p0x == p0x.floor() && p0x != 0.0 {
        p0x -= epsilon;
    }
    if p1x == p1x.floor() && p1x != 0.0 {
        p1x -= epsilon;
    }
    if !is_down {
        let tmp_x = p0x;
        let tmp_y = p0y;
        p0x = p1x;
        p0y = p1y;
        p1x = tmp_x;
        p1y = tmp_y;
    }

    segment_p0x[dst] = p0x;
    segment_p0y[dst] = p0y;
    segment_p1x[dst] = p1x;
    segment_p1y[dst] = p1y;
    segment_y_edge[dst] = y_edge;
}
