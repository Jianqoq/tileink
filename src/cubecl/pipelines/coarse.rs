use ::cubecl::prelude::*;

use crate::cubecl::{
    renderer::{CoarseBuffers, ScanBuffers, SceneBuffers},
    types::{COARSE_CHUNK_SIZE, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CubeBufferLengths},
};

pub(crate) const TILE_WORKGROUP_SIZE: u32 = 256;

pub(crate) struct CoarsePipeline;

impl CoarsePipeline {
    pub(crate) fn run<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        coarse: &mut CoarseBuffers,
        lengths: CubeBufferLengths,
    ) {
        let tile_count = lengths.tile_count as u32;
        let chunk_count = lengths.coarse_chunk_count as u32;
        if tile_count == 0 || chunk_count == 0 {
            return;
        }

        coarse_count::launch::<R>(
            client,
            CubeCount::Static(tile_count, 1, 1),
            CubeDim::new_1d(TILE_WORKGROUP_SIZE),
            TILE_WORKGROUP_SIZE as usize,
            tile_count,
            lengths.tiles_width as u32,
            lengths.tiles_height as u32,
            lengths.draw_count as u32,
            unsafe { scene.draw_path_ids.arg() },
            unsafe { scene.draw_tags.arg() },
            unsafe { scene.draw_pixel_x0.arg() },
            unsafe { scene.draw_pixel_y0.arg() },
            unsafe { scene.draw_pixel_x1.arg() },
            unsafe { scene.draw_pixel_y1.arg() },
            unsafe { scene.backdrop_data_offsets.arg() },
            unsafe { scene.backdrop_tile_x0.arg() },
            unsafe { scene.backdrop_tile_y0.arg() },
            unsafe { scene.backdrop_tile_x1.arg() },
            unsafe { scene.backdrop_tile_y1.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.tile_segment_range_starts.arg() },
            unsafe { scan.tile_segment_range_ends.arg() },
            unsafe { coarse.tile_ptcl_counts.arg() },
        );

        coarse_prefix_chunks::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(COARSE_CHUNK_SIZE),
            COARSE_CHUNK_SIZE as usize,
            tile_count,
            unsafe { coarse.tile_ptcl_counts.arg() },
            unsafe { coarse.tile_ptcl_range_starts.arg() },
            unsafe { coarse.tile_ptcl_range_ends.arg() },
            unsafe { coarse.chunk_totals.arg() },
        );

        coarse_chunk_offsets::launch::<R>(
            client,
            CubeCount::Static(1, 1, 1),
            CubeDim::new_1d(1),
            chunk_count,
            unsafe { coarse.chunk_totals.arg() },
            unsafe { coarse.chunk_offsets.arg() },
        );

        coarse_apply_chunk_offsets::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(COARSE_CHUNK_SIZE),
            COARSE_CHUNK_SIZE as usize,
            tile_count,
            unsafe { coarse.chunk_offsets.arg() },
            unsafe { coarse.tile_ptcl_range_starts.arg() },
            unsafe { coarse.tile_ptcl_range_ends.arg() },
        );

        if lengths.draw_count == 0 || lengths.coarse_ptcl_capacity == 0 {
            return;
        }

        coarse_emit::launch::<R>(
            client,
            CubeCount::Static(tile_count, 1, 1),
            CubeDim::new_1d(TILE_WORKGROUP_SIZE),
            TILE_WORKGROUP_SIZE as usize,
            tile_count,
            lengths.tiles_width as u32,
            lengths.tiles_height as u32,
            lengths.draw_count as u32,
            lengths.coarse_ptcl_capacity as u32,
            unsafe { scene.draw_path_ids.arg() },
            unsafe { scene.draw_tags.arg() },
            unsafe { scene.draw_fill_rules.arg() },
            unsafe { scene.draw_solid_color_fast_paths.arg() },
            unsafe { scene.draw_brush_colors.arg() },
            unsafe { scene.draw_pixel_x0.arg() },
            unsafe { scene.draw_pixel_y0.arg() },
            unsafe { scene.draw_pixel_x1.arg() },
            unsafe { scene.draw_pixel_y1.arg() },
            unsafe { scene.backdrop_data_offsets.arg() },
            unsafe { scene.backdrop_tile_x0.arg() },
            unsafe { scene.backdrop_tile_y0.arg() },
            unsafe { scene.backdrop_tile_x1.arg() },
            unsafe { scene.backdrop_tile_y1.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.tile_segment_range_starts.arg() },
            unsafe { scan.tile_segment_range_ends.arg() },
            unsafe { coarse.tile_ptcl_range_starts.arg() },
            unsafe { coarse.ptcl_tags.arg() },
            unsafe { coarse.ptcl_backdrops.arg() },
            unsafe { coarse.ptcl_fill_rules.arg() },
            unsafe { coarse.ptcl_segment_starts.arg() },
            unsafe { coarse.ptcl_segment_ends.arg() },
            unsafe { coarse.ptcl_colors.arg() },
        );
    }
}

#[cube(launch)]
fn coarse_count(
    #[comptime] workgroup_size: usize,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_count: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    tile_ptcl_counts: &mut Array<u32>,
) {
    let tile_ix = CUBE_POS as u32;
    if tile_ix >= tile_count {
        terminate!();
    }

    let invalid = u32::new(-1);
    let tile_x = tile_ix % tiles_width;
    let tile_y = tile_ix / tiles_width;
    let mut count = 0u32;
    let mut draw_ix = UNIT_POS;
    while draw_ix < draw_count {
        let backdrop_ix = draw_backdrop_ix(
            draw_ix,
            tile_x,
            tile_y,
            tiles_width,
            tiles_height,
            draw_path_ids,
            draw_tags,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
        );
        if backdrop_ix != invalid {
            let i = backdrop_ix as usize;
            if segment_starts[i] != segment_ends[i] || backdrops[i].load() != 0 {
                count += 1;
            }
        }
        draw_ix += workgroup_size as u32;
    }

    let plane_total = plane_sum(count);
    let mut plane_totals = SharedMemory::<u32>::new(workgroup_size);
    if UNIT_POS_PLANE == 0 {
        plane_totals[PLANE_POS as usize] = plane_total;
    }
    sync_cube();

    if UNIT_POS == 0 {
        let plane_count = CUBE_DIM.div_ceil(PLANE_DIM);
        let mut tile_count = 0u32;
        let mut plane_ix = 0u32;
        while plane_ix < plane_count {
            tile_count += plane_totals[plane_ix as usize];
            plane_ix += 1;
        }
        tile_ptcl_counts[tile_ix as usize] = if tile_count > 0 {
            tile_count + 1
        } else {
            tile_count
        };
    }
}

#[cube(launch)]
fn coarse_prefix_chunks(
    #[comptime] chunk_size: usize,
    tile_count: u32,
    tile_ptcl_counts: &Array<u32>,
    range_starts: &mut Array<u32>,
    range_ends: &mut Array<u32>,
    chunk_totals: &mut Array<u32>,
) {
    let chunk_i = CUBE_POS;
    let chunk_ix = CUBE_POS as u32;
    let lane = UNIT_POS as usize;
    let chunk_offset = chunk_ix * chunk_size as u32;
    let chunk_len = (tile_count - chunk_offset).min(chunk_size as u32);
    let count = if lane < chunk_len as usize {
        tile_ptcl_counts[(chunk_offset + lane as u32) as usize]
    } else {
        u32::new(0)
    };

    let mut shared = SharedMemory::<u32>::new(chunk_size);
    shared[lane] = count;
    sync_cube();

    let step = RuntimeCell::<u32>::new(1);
    while step.read() < chunk_size as u32 {
        let step_value = step.read();
        let step_usize = step_value as usize;
        let ix = (lane + 1) * step_usize * 2 - 1;
        if ix < chunk_size {
            shared[ix] += shared[ix - step_usize];
        }
        sync_cube();
        step.store(step_value * 2);
    }

    if lane == 0 {
        chunk_totals[chunk_i] = shared[chunk_size - 1];
        shared[chunk_size - 1] = 0;
    }
    sync_cube();

    let step = RuntimeCell::<u32>::new((chunk_size as u32) / 2);
    while step.read() > 0 {
        let step_value = step.read();
        let step_usize = step_value as usize;
        let ix = (lane + 1) * step_usize * 2 - 1;
        if ix < chunk_size {
            let left = ix - step_usize;
            let previous_left = shared[left];
            shared[left] = shared[ix];
            shared[ix] += previous_left;
        }
        sync_cube();
        step.store(step_value / 2);
    }

    if lane < chunk_len as usize {
        let ix = (chunk_offset + lane as u32) as usize;
        let start = shared[lane];
        range_starts[ix] = start;
        range_ends[ix] = start + count;
    }
}

#[cube(launch)]
fn coarse_chunk_offsets(
    chunk_count: u32,
    chunk_totals: &Array<u32>,
    chunk_offsets: &mut Array<u32>,
) {
    let mut carry = 0u32;
    let mut chunk_ix = 0u32;
    while chunk_ix < chunk_count {
        let ix = chunk_ix as usize;
        chunk_offsets[ix] = carry;
        carry += chunk_totals[ix];
        chunk_ix += 1;
    }
}

#[cube(launch)]
fn coarse_apply_chunk_offsets(
    #[comptime] chunk_size: usize,
    tile_count: u32,
    chunk_offsets: &Array<u32>,
    range_starts: &mut Array<u32>,
    range_ends: &mut Array<u32>,
) {
    let chunk_ix = CUBE_POS as u32;
    let lane = UNIT_POS;
    let tile_ix = chunk_ix * chunk_size as u32 + lane;
    if tile_ix >= tile_count {
        terminate!();
    }

    let i = tile_ix as usize;
    let offset = chunk_offsets[chunk_ix as usize];
    range_starts[i] += offset;
    range_ends[i] += offset;
}

#[cube(launch)]
fn coarse_emit(
    #[comptime] workgroup_size: usize,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_count: u32,
    ptcl_capacity: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_solid_color_fast_paths: &Array<u32>,
    draw_brush_colors: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    tile_ptcl_range_starts: &Array<u32>,
    ptcl_tags: &mut Array<u32>,
    ptcl_backdrops: &mut Array<i32>,
    ptcl_fill_rules: &mut Array<u32>,
    ptcl_segment_starts: &mut Array<u32>,
    ptcl_segment_ends: &mut Array<u32>,
    ptcl_colors: &mut Array<u32>,
) {
    let tile_ix = CUBE_POS as u32;
    if tile_ix >= tile_count {
        terminate!();
    }

    let invalid = u32::new(-1);
    let tile_x = tile_ix % tiles_width;
    let tile_y = tile_ix / tiles_width;
    let mut cursor = tile_ptcl_range_starts[tile_ix as usize];
    let start = cursor;

    let mut plane_totals = SharedMemory::<u32>::new(workgroup_size);
    let mut chunk_start = 0u32;
    while chunk_start < draw_count {
        let draw_ix = chunk_start + UNIT_POS;
        let mut valid = 0u32;
        let mut ptcl_tag = u32::new(1);
        let mut ptcl_backdrop = i32::new(0);
        let mut ptcl_fill_rule = 0u32;
        let mut ptcl_segment_start = 0u32;
        let mut ptcl_segment_end = 0u32;
        let mut ptcl_color = 0u32;

        if draw_ix < draw_count {
            let backdrop_ix = draw_backdrop_ix(
                draw_ix,
                tile_x,
                tile_y,
                tiles_width,
                tiles_height,
                draw_path_ids,
                draw_tags,
                draw_pixel_x0,
                draw_pixel_y0,
                draw_pixel_x1,
                draw_pixel_y1,
                backdrop_data_offsets,
                backdrop_tile_x0,
                backdrop_tile_y0,
                backdrop_tile_x1,
                backdrop_tile_y1,
            );

            if backdrop_ix != invalid {
                let backdrop_i = backdrop_ix as usize;
                let segment_start = segment_starts[backdrop_i];
                let segment_end = segment_ends[backdrop_i];
                let backdrop = backdrops[backdrop_i].load();
                if segment_start != segment_end || backdrop != 0 {
                    let draw_i = draw_ix as usize;
                    let draw_tag = draw_tags[draw_i];
                    if draw_tag == 1 {
                        ptcl_tag = u32::new(3);
                    } else {
                        let solid_color_fast_path = draw_solid_color_fast_paths[draw_i] == 1;
                        let empty_segment_range = segment_start == segment_end;
                        if solid_color_fast_path && empty_segment_range {
                            ptcl_tag = u32::new(2);
                        }
                    }
                    valid = 1;
                    ptcl_backdrop = backdrop;
                    ptcl_fill_rule = draw_fill_rules[draw_i];
                    ptcl_segment_start = segment_start;
                    ptcl_segment_end = segment_end;
                    ptcl_color = draw_brush_colors[draw_i];
                }
            }
        }

        // Plane scans avoid the 8-step shared-memory scan for a 256-lane chunk.
        let in_plane_exclusive = plane_exclusive_sum(valid);
        let plane_total = plane_sum(valid);
        if UNIT_POS_PLANE == 0 {
            plane_totals[PLANE_POS as usize] = plane_total;
        }
        sync_cube();

        let mut plane_offset = 0u32;
        let mut plane_ix = 0u32;
        while plane_ix < PLANE_POS {
            plane_offset += plane_totals[plane_ix as usize];
            plane_ix += 1;
        }
        let particle_offset = plane_offset + in_plane_exclusive;
        sync_cube();

        if UNIT_POS == 0 {
            let plane_count = CUBE_DIM.div_ceil(PLANE_DIM);
            let mut emitted = 0u32;
            let mut plane_ix = 0u32;
            while plane_ix < plane_count {
                emitted += plane_totals[plane_ix as usize];
                plane_ix += 1;
            }
            plane_totals[0] = emitted;
        }
        if valid == 1 {
            store_particle(
                cursor + particle_offset,
                ptcl_capacity,
                ptcl_tag,
                ptcl_backdrop,
                ptcl_fill_rule,
                ptcl_segment_start,
                ptcl_segment_end,
                ptcl_color,
                ptcl_tags,
                ptcl_backdrops,
                ptcl_fill_rules,
                ptcl_segment_starts,
                ptcl_segment_ends,
                ptcl_colors,
            );
        }
        sync_cube();
        let emitted = plane_totals[0];
        cursor += emitted;
        sync_cube();
        chunk_start += workgroup_size as u32;
    }

    if UNIT_POS == 0 && cursor > start {
        store_particle(
            cursor,
            ptcl_capacity,
            u32::new(0),
            0,
            0,
            0,
            0,
            0,
            ptcl_tags,
            ptcl_backdrops,
            ptcl_fill_rules,
            ptcl_segment_starts,
            ptcl_segment_ends,
            ptcl_colors,
        );
    }
}

#[cube]
fn draw_backdrop_ix(
    draw_ix: u32,
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
) -> u32 {
    let invalid = u32::new(-1);
    let draw_i = draw_ix as usize;
    let path_id = draw_path_ids[draw_i];
    let draw_tag = draw_tags[draw_i];
    let mut result = invalid;

    if path_id != invalid && (draw_tag == CUBE_DRAW_BRUSH || draw_tag == CUBE_DRAW_CLIP) {
        let draw_x0 = pixel_tile_min(draw_pixel_x0[draw_i], tiles_width);
        let draw_y0 = pixel_tile_min(draw_pixel_y0[draw_i], tiles_height);
        let draw_x1 = pixel_tile_max(draw_pixel_x1[draw_i], tiles_width);
        let draw_y1 = pixel_tile_max(draw_pixel_y1[draw_i], tiles_height);
        if tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1 {
            let path_i = path_id as usize;
            if path_i < backdrop_data_offsets.len() {
                let bx0 = backdrop_tile_x0[path_i];
                let by0 = backdrop_tile_y0[path_i];
                let bx1 = backdrop_tile_x1[path_i];
                let by1 = backdrop_tile_y1[path_i];
                let stride = bx1 - bx0;
                if stride > 0 && tile_x >= bx0 && tile_x < bx1 && tile_y >= by0 && tile_y < by1 {
                    let local_ix = (tile_y - by0) * stride + tile_x - bx0;
                    result = backdrop_data_offsets[path_i] + local_ix;
                }
            }
        }
    }

    result
}

#[cube]
fn pixel_tile_min(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32 / 16).min(limit);
    }
    tile
}

#[cube]
fn pixel_tile_max(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32).div_ceil(16).min(limit);
    }
    tile
}

#[cube]
fn store_particle(
    dst: u32,
    capacity: u32,
    tag: u32,
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    color: u32,
    ptcl_tags: &mut Array<u32>,
    ptcl_backdrops: &mut Array<i32>,
    ptcl_fill_rules: &mut Array<u32>,
    ptcl_segment_starts: &mut Array<u32>,
    ptcl_segment_ends: &mut Array<u32>,
    ptcl_colors: &mut Array<u32>,
) {
    if dst < capacity {
        let i = dst as usize;
        ptcl_tags[i] = tag;
        ptcl_backdrops[i] = backdrop;
        ptcl_fill_rules[i] = fill_rule;
        ptcl_segment_starts[i] = segment_start;
        ptcl_segment_ends[i] = segment_end;
        ptcl_colors[i] = color;
    }
}
