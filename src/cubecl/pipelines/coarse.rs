use ::cubecl::prelude::*;

use crate::cubecl::{
    renderer::{CoarseBuffers, ScanBuffers, SceneBuffers},
    types::{
        COARSE_CHUNK_SIZE, CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_ISOLATE,
        CUBE_DRAW_OPACITY, CUBE_DRAW_PATH_GLYPH, CUBE_LAYER_BLEND, CUBE_LAYER_CLIP,
        CUBE_LAYER_OPACITY, CUBE_PTCL_BEGIN_BLEND, CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_BEGIN_OPACITY,
        CUBE_PTCL_BEGIN_SDF_CLIP, CUBE_PTCL_COLOR, CUBE_PTCL_END, CUBE_PTCL_END_BLEND,
        CUBE_PTCL_END_CLIP, CUBE_PTCL_END_OPACITY, CUBE_PTCL_FILL, CUBE_PTCL_GLYPH,
        CUBE_PTCL_PATH_GLYPH, CUBE_PTCL_SDF, CUBE_SDF_NONE, CubeBufferLengths,
    },
};

pub(crate) const TILE_WORKGROUP_SIZE: u32 = 256;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CoarseBatch {
    pub(crate) draw_start: u32,
    pub(crate) draw_end: u32,
    pub(crate) layer_stack_start: u32,
    pub(crate) layer_stack_end: u32,
}

pub(crate) struct CoarsePipeline;

impl CoarsePipeline {
    pub(crate) fn run<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        coarse: &mut CoarseBuffers,
        lengths: CubeBufferLengths,
        batch: CoarseBatch,
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
            batch.draw_start,
            batch.draw_end,
            batch.layer_stack_start,
            batch.layer_stack_end,
            unsafe { scene.draw_path_ids.arg() },
            unsafe { scene.draw_glyph_run_ids.arg() },
            unsafe { scene.glyph_run_starts.arg() },
            unsafe { scene.glyph_run_counts.arg() },
            unsafe { scene.glyph_image_ids.arg() },
            unsafe { scene.glyph_x.arg() },
            unsafe { scene.glyph_y.arg() },
            unsafe { scene.glyph_image_left.arg() },
            unsafe { scene.glyph_image_top.arg() },
            unsafe { scene.glyph_image_width.arg() },
            unsafe { scene.glyph_image_height.arg() },
            unsafe { scene.draw_tags.arg() },
            unsafe { scene.draw_pixel_x0.arg() },
            unsafe { scene.draw_pixel_y0.arg() },
            unsafe { scene.draw_pixel_x1.arg() },
            unsafe { scene.draw_pixel_y1.arg() },
            unsafe { scene.draw_sdf_kinds.arg() },
            unsafe { scene.backdrop_data_offsets.arg() },
            unsafe { scene.backdrop_tile_x0.arg() },
            unsafe { scene.backdrop_tile_y0.arg() },
            unsafe { scene.backdrop_tile_x1.arg() },
            unsafe { scene.backdrop_tile_y1.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.tile_segment_range_starts.arg() },
            unsafe { scan.tile_segment_range_ends.arg() },
            unsafe { scene.plan_layer_stack_tags.arg() },
            unsafe { scene.plan_layer_stack_draws.arg() },
            unsafe { coarse.tile_ptcl_counts.arg() },
            unsafe { coarse.tile_glyph_counts.arg() },
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

        coarse_prefix_chunks::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(COARSE_CHUNK_SIZE),
            COARSE_CHUNK_SIZE as usize,
            tile_count,
            unsafe { coarse.tile_glyph_counts.arg() },
            unsafe { coarse.tile_glyph_range_starts.arg() },
            unsafe { coarse.tile_glyph_range_ends.arg() },
            unsafe { coarse.glyph_chunk_totals.arg() },
        );

        coarse_chunk_offsets::launch::<R>(
            client,
            CubeCount::Static(1, 1, 1),
            CubeDim::new_1d(1),
            chunk_count,
            unsafe { coarse.glyph_chunk_totals.arg() },
            unsafe { coarse.glyph_chunk_offsets.arg() },
        );

        coarse_apply_chunk_offsets::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(COARSE_CHUNK_SIZE),
            COARSE_CHUNK_SIZE as usize,
            tile_count,
            unsafe { coarse.glyph_chunk_offsets.arg() },
            unsafe { coarse.tile_glyph_range_starts.arg() },
            unsafe { coarse.tile_glyph_range_ends.arg() },
        );

        if batch.draw_start >= batch.draw_end || lengths.coarse_ptcl_capacity == 0 {
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
            batch.draw_start,
            batch.draw_end,
            batch.layer_stack_start,
            batch.layer_stack_end,
            lengths.coarse_ptcl_capacity as u32,
            lengths.coarse_glyph_capacity as u32,
            unsafe { scene.draw_path_ids.arg() },
            unsafe { scene.draw_glyph_run_ids.arg() },
            unsafe { scene.glyph_run_starts.arg() },
            unsafe { scene.glyph_run_counts.arg() },
            unsafe { scene.glyph_image_ids.arg() },
            unsafe { scene.glyph_x.arg() },
            unsafe { scene.glyph_y.arg() },
            unsafe { scene.glyph_image_left.arg() },
            unsafe { scene.glyph_image_top.arg() },
            unsafe { scene.glyph_image_width.arg() },
            unsafe { scene.glyph_image_height.arg() },
            unsafe { scene.draw_tags.arg() },
            unsafe { scene.draw_fill_rules.arg() },
            unsafe { scene.draw_solid_color_fast_paths.arg() },
            unsafe { scene.draw_brush_colors.arg() },
            unsafe { scene.draw_pixel_x0.arg() },
            unsafe { scene.draw_pixel_y0.arg() },
            unsafe { scene.draw_pixel_x1.arg() },
            unsafe { scene.draw_pixel_y1.arg() },
            unsafe { scene.draw_sdf_kinds.arg() },
            unsafe { scene.backdrop_data_offsets.arg() },
            unsafe { scene.backdrop_tile_x0.arg() },
            unsafe { scene.backdrop_tile_y0.arg() },
            unsafe { scene.backdrop_tile_x1.arg() },
            unsafe { scene.backdrop_tile_y1.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.tile_segment_range_starts.arg() },
            unsafe { scan.tile_segment_range_ends.arg() },
            unsafe { coarse.tile_ptcl_range_starts.arg() },
            unsafe { coarse.tile_ptcl_range_ends.arg() },
            unsafe { coarse.tile_glyph_range_starts.arg() },
            unsafe { coarse.tile_glyph_range_ends.arg() },
            unsafe { scene.plan_layer_stack_tags.arg() },
            unsafe { scene.plan_layer_stack_draws.arg() },
            unsafe { scene.plan_layer_stack_payloads.arg() },
            unsafe { coarse.ptcl_tags.arg() },
            unsafe { coarse.ptcl_backdrops.arg() },
            unsafe { coarse.ptcl_fill_rules.arg() },
            unsafe { coarse.ptcl_segment_starts.arg() },
            unsafe { coarse.ptcl_segment_ends.arg() },
            unsafe { coarse.ptcl_colors.arg() },
            unsafe { coarse.glyph_indices.arg() },
        );
    }
}

#[cube(launch)]
fn coarse_count(
    #[comptime] workgroup_size: usize,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_start: u32,
    draw_end: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    draw_path_ids: &Array<u32>,
    draw_glyph_run_ids: &Array<u32>,
    glyph_run_starts: &Array<u32>,
    glyph_run_counts: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_kinds: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    tile_ptcl_counts: &mut Array<u32>,
    tile_glyph_counts: &mut Array<u32>,
) {
    let tile_ix = CUBE_POS as u32;
    if tile_ix >= tile_count {
        terminate!();
    }

    let invalid = u32::new(-1);
    let tile_x = tile_ix % tiles_width;
    let tile_y = tile_ix / tiles_width;
    let wrapper_count = active_stack_count(
        tile_x,
        tile_y,
        tiles_width,
        tiles_height,
        layer_stack_start,
        layer_stack_end,
        layer_stack_tags,
        layer_stack_draws,
        draw_path_ids,
        draw_tags,
        draw_pixel_x0,
        draw_pixel_y0,
        draw_pixel_x1,
        draw_pixel_y1,
        draw_sdf_kinds,
        backdrop_data_offsets,
        backdrop_tile_x0,
        backdrop_tile_y0,
        backdrop_tile_x1,
        backdrop_tile_y1,
        backdrops,
        segment_starts,
        segment_ends,
    );
    let mut count = 0u32;

    if UNIT_POS == 0 {
        tile_glyph_counts[tile_ix as usize] = 0;
    }

    if wrapper_count != invalid {
        let mut draw_ix = draw_start + UNIT_POS;
        let mut glyph_count = 0u32;
        while draw_ix < draw_end {
            let draw_i = draw_ix as usize;
            let draw_tag = draw_tags[draw_i];
            if draw_glyph_run_ids[draw_i] != invalid {
                if draw_tag == CUBE_DRAW_BRUSH
                    && draw_tile_hit(
                        draw_i,
                        tile_x,
                        tile_y,
                        tiles_width,
                        tiles_height,
                        draw_pixel_x0,
                        draw_pixel_y0,
                        draw_pixel_x1,
                        draw_pixel_y1,
                    )
                {
                    let tile_glyphs = count_tile_glyphs_for_run(
                        draw_glyph_run_ids[draw_i],
                        tile_x,
                        tile_y,
                        glyph_run_starts,
                        glyph_run_counts,
                        glyph_image_ids,
                        glyph_x,
                        glyph_y,
                        glyph_image_left,
                        glyph_image_top,
                        glyph_image_width,
                        glyph_image_height,
                    );
                    if tile_glyphs > 0 {
                        count += 1;
                        glyph_count += tile_glyphs;
                    }
                }
            } else if draw_sdf_kinds[draw_i] != CUBE_SDF_NONE {
                if draw_tag == CUBE_DRAW_BRUSH
                    && draw_tile_hit(
                        draw_i,
                        tile_x,
                        tile_y,
                        tiles_width,
                        tiles_height,
                        draw_pixel_x0,
                        draw_pixel_y0,
                        draw_pixel_x1,
                        draw_pixel_y1,
                    )
                {
                    count += 1;
                }
            } else {
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
                    if (draw_tag == CUBE_DRAW_BRUSH
                        || draw_tag == CUBE_DRAW_PATH_GLYPH
                        || draw_tag == CUBE_DRAW_CLIP)
                        && (segment_starts[i] != segment_ends[i] || backdrops[i].load() != 0)
                    {
                        count += 1;
                    }
                }
            }
            draw_ix += workgroup_size as u32;
        }
        let glyph_plane_total = plane_sum(glyph_count);
        let mut glyph_plane_totals = SharedMemory::<u32>::new(workgroup_size);
        if UNIT_POS_PLANE == 0 {
            glyph_plane_totals[PLANE_POS as usize] = glyph_plane_total;
        }
        sync_cube();

        if UNIT_POS == 0 {
            let plane_count = CUBE_DIM.div_ceil(PLANE_DIM);
            let mut tile_glyph_count = 0u32;
            let mut plane_ix = 0u32;
            while plane_ix < plane_count {
                tile_glyph_count += glyph_plane_totals[plane_ix as usize];
                plane_ix += 1;
            }
            tile_glyph_counts[tile_ix as usize] = tile_glyph_count;
        }
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
        let mut stored_count = tile_count;
        if tile_count > 0 {
            stored_count = tile_count + wrapper_count * 2 + 1;
        }
        tile_ptcl_counts[tile_ix as usize] = stored_count;
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
    let mut count = u32::new(0);
    if lane < chunk_len as usize {
        count = tile_ptcl_counts[(chunk_offset + lane as u32) as usize];
    }

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
    draw_start: u32,
    draw_end: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    ptcl_capacity: u32,
    glyph_capacity: u32,
    draw_path_ids: &Array<u32>,
    draw_glyph_run_ids: &Array<u32>,
    glyph_run_starts: &Array<u32>,
    glyph_run_counts: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_solid_color_fast_paths: &Array<u32>,
    draw_brush_colors: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_kinds: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    tile_ptcl_range_starts: &Array<u32>,
    tile_ptcl_range_ends: &Array<u32>,
    tile_glyph_range_starts: &Array<u32>,
    tile_glyph_range_ends: &Array<u32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    ptcl_tags: &mut Array<u32>,
    ptcl_backdrops: &mut Array<i32>,
    ptcl_fill_rules: &mut Array<u32>,
    ptcl_segment_starts: &mut Array<u32>,
    ptcl_segment_ends: &mut Array<u32>,
    ptcl_colors: &mut Array<u32>,
    glyph_indices: &mut Array<u32>,
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
    let range_end = tile_ptcl_range_ends[tile_ix as usize];
    let mut glyph_cursor = tile_glyph_range_starts[tile_ix as usize];
    let glyph_range_end = tile_glyph_range_ends[tile_ix as usize];
    if start >= range_end {
        terminate!();
    }

    let wrapper_count = active_stack_count(
        tile_x,
        tile_y,
        tiles_width,
        tiles_height,
        layer_stack_start,
        layer_stack_end,
        layer_stack_tags,
        layer_stack_draws,
        draw_path_ids,
        draw_tags,
        draw_pixel_x0,
        draw_pixel_y0,
        draw_pixel_x1,
        draw_pixel_y1,
        draw_sdf_kinds,
        backdrop_data_offsets,
        backdrop_tile_x0,
        backdrop_tile_y0,
        backdrop_tile_x1,
        backdrop_tile_y1,
        backdrops,
        segment_starts,
        segment_ends,
    );
    if wrapper_count == invalid {
        terminate!();
    }

    if UNIT_POS == 0 {
        emit_active_stack_begins(
            cursor,
            ptcl_capacity,
            tile_x,
            tile_y,
            tiles_width,
            tiles_height,
            layer_stack_start,
            layer_stack_end,
            layer_stack_tags,
            layer_stack_draws,
            layer_stack_payloads,
            draw_path_ids,
            draw_tags,
            draw_fill_rules,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            draw_sdf_kinds,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
            backdrops,
            segment_starts,
            segment_ends,
            ptcl_tags,
            ptcl_backdrops,
            ptcl_fill_rules,
            ptcl_segment_starts,
            ptcl_segment_ends,
            ptcl_colors,
        );
    }
    cursor += wrapper_count;
    sync_cube();

    let mut plane_totals = SharedMemory::<u32>::new(workgroup_size);
    let mut glyph_plane_totals = SharedMemory::<u32>::new(workgroup_size);
    let mut chunk_start = draw_start;
    while chunk_start < draw_end {
        let draw_ix = chunk_start + UNIT_POS;
        let mut valid = 0u32;
        let mut glyph_count = 0u32;
        let mut ptcl_tag = u32::new(CUBE_PTCL_FILL as i64);
        let mut ptcl_backdrop = i32::new(0);
        let mut ptcl_fill_rule = 0u32;
        let mut ptcl_segment_start = 0u32;
        let mut ptcl_segment_end = 0u32;
        let mut ptcl_color = 0u32;

        if draw_ix < draw_end {
            let draw_i = draw_ix as usize;
            let draw_tag = draw_tags[draw_i];
            if draw_glyph_run_ids[draw_i] != invalid {
                if draw_tag == CUBE_DRAW_BRUSH
                    && draw_tile_hit(
                        draw_i,
                        tile_x,
                        tile_y,
                        tiles_width,
                        tiles_height,
                        draw_pixel_x0,
                        draw_pixel_y0,
                        draw_pixel_x1,
                        draw_pixel_y1,
                    )
                {
                    glyph_count = count_tile_glyphs_for_run(
                        draw_glyph_run_ids[draw_i],
                        tile_x,
                        tile_y,
                        glyph_run_starts,
                        glyph_run_counts,
                        glyph_image_ids,
                        glyph_x,
                        glyph_y,
                        glyph_image_left,
                        glyph_image_top,
                        glyph_image_width,
                        glyph_image_height,
                    );
                    if glyph_count > 0 {
                        valid = 1;
                        ptcl_tag = u32::new(CUBE_PTCL_GLYPH as i64);
                        ptcl_color = draw_ix;
                    }
                }
            } else if draw_sdf_kinds[draw_i] != CUBE_SDF_NONE {
                if draw_tag == CUBE_DRAW_BRUSH
                    && draw_tile_hit(
                        draw_i,
                        tile_x,
                        tile_y,
                        tiles_width,
                        tiles_height,
                        draw_pixel_x0,
                        draw_pixel_y0,
                        draw_pixel_x1,
                        draw_pixel_y1,
                    )
                {
                    valid = 1;
                    ptcl_tag = u32::new(CUBE_PTCL_SDF as i64);
                    ptcl_segment_start = draw_ix;
                    ptcl_color = draw_ix;
                }
            } else {
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
                    if (draw_tag == CUBE_DRAW_BRUSH
                        || draw_tag == CUBE_DRAW_PATH_GLYPH
                        || draw_tag == CUBE_DRAW_CLIP)
                        && (segment_start != segment_end || backdrop != 0)
                    {
                        if draw_tag == CUBE_DRAW_CLIP {
                            ptcl_tag = u32::new(CUBE_PTCL_BEGIN_CLIP as i64);
                        } else if draw_tag == CUBE_DRAW_PATH_GLYPH {
                            ptcl_tag = u32::new(CUBE_PTCL_PATH_GLYPH as i64);
                        } else {
                            let solid_color_fast_path = draw_solid_color_fast_paths[draw_i] == 1;
                            let empty_segment_range = segment_start == segment_end;
                            if solid_color_fast_path && empty_segment_range {
                                ptcl_tag = u32::new(CUBE_PTCL_COLOR as i64);
                            }
                        }
                        valid = 1;
                        ptcl_backdrop = backdrop;
                        ptcl_fill_rule = draw_fill_rules[draw_i];
                        ptcl_segment_start = segment_start;
                        ptcl_segment_end = segment_end;
                        if ptcl_tag == CUBE_PTCL_COLOR {
                            ptcl_color = draw_brush_colors[draw_i];
                        } else if draw_tag == CUBE_DRAW_BRUSH || draw_tag == CUBE_DRAW_PATH_GLYPH {
                            ptcl_color = draw_ix;
                        }
                    }
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

        let glyph_in_plane_exclusive = plane_exclusive_sum(glyph_count);
        let glyph_plane_total = plane_sum(glyph_count);
        if UNIT_POS_PLANE == 0 {
            glyph_plane_totals[PLANE_POS as usize] = glyph_plane_total;
        }
        sync_cube();

        let mut glyph_plane_offset = 0u32;
        let mut glyph_plane_ix = 0u32;
        while glyph_plane_ix < PLANE_POS {
            glyph_plane_offset += glyph_plane_totals[glyph_plane_ix as usize];
            glyph_plane_ix += 1;
        }
        let glyph_offset = glyph_plane_offset + glyph_in_plane_exclusive;
        sync_cube();

        if UNIT_POS == 0 {
            let plane_count = CUBE_DIM.div_ceil(PLANE_DIM);
            let mut emitted = 0u32;
            let mut emitted_glyphs = 0u32;
            let mut plane_ix = 0u32;
            while plane_ix < plane_count {
                emitted += plane_totals[plane_ix as usize];
                emitted_glyphs += glyph_plane_totals[plane_ix as usize];
                plane_ix += 1;
            }
            plane_totals[0] = emitted;
            glyph_plane_totals[0] = emitted_glyphs;
        }
        if valid == 1 {
            if ptcl_tag == CUBE_PTCL_GLYPH {
                ptcl_segment_start = glyph_cursor + glyph_offset;
                ptcl_segment_end = ptcl_segment_start + glyph_count;
                if ptcl_segment_end <= glyph_range_end {
                    store_tile_glyphs_for_run(
                        ptcl_segment_start,
                        glyph_capacity,
                        draw_glyph_run_ids[draw_ix as usize],
                        tile_x,
                        tile_y,
                        glyph_run_starts,
                        glyph_run_counts,
                        glyph_image_ids,
                        glyph_x,
                        glyph_y,
                        glyph_image_left,
                        glyph_image_top,
                        glyph_image_width,
                        glyph_image_height,
                        glyph_indices,
                    );
                }
            }
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
        let emitted_glyphs = glyph_plane_totals[0];
        cursor += emitted;
        glyph_cursor += emitted_glyphs;
        sync_cube();
        chunk_start += workgroup_size as u32;
    }

    if UNIT_POS == 0 {
        emit_active_stack_ends(
            cursor,
            ptcl_capacity,
            layer_stack_start,
            layer_stack_end,
            layer_stack_tags,
            ptcl_tags,
            ptcl_backdrops,
            ptcl_fill_rules,
            ptcl_segment_starts,
            ptcl_segment_ends,
            ptcl_colors,
        );
        store_particle(
            cursor + wrapper_count,
            ptcl_capacity,
            u32::new(CUBE_PTCL_END as i64),
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
fn active_stack_count(
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_kinds: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
) -> u32 {
    let invalid = u32::new(-1);
    let mut count = 0u32;
    let mut valid = u32::new(1);
    let mut stack_ix = layer_stack_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        if valid == 1 {
            let layer_tag = layer_stack_tags[stack_i];
            if layer_tag != CUBE_LAYER_CLIP
                && layer_tag != CUBE_LAYER_OPACITY
                && layer_tag != CUBE_LAYER_BLEND
            {
                valid = 0;
            } else {
                let draw_ix = layer_stack_draws[stack_i];
                let draw_i = draw_ix as usize;
                if draw_sdf_kinds[draw_i] != CUBE_SDF_NONE {
                    if draw_tile_hit(
                        draw_i,
                        tile_x,
                        tile_y,
                        tiles_width,
                        tiles_height,
                        draw_pixel_x0,
                        draw_pixel_y0,
                        draw_pixel_x1,
                        draw_pixel_y1,
                    ) {
                        count += 1;
                    } else {
                        valid = 0;
                    }
                } else {
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
                    if backdrop_ix == invalid {
                        valid = 0;
                    } else {
                        let backdrop_i = backdrop_ix as usize;
                        if segment_starts[backdrop_i] == segment_ends[backdrop_i]
                            && backdrops[backdrop_i].load() == 0
                        {
                            valid = 0;
                        } else {
                            count += 1;
                        }
                    }
                }
            }
        }
        stack_ix += 1;
    }
    let mut out = invalid;
    if valid == 1 {
        out = count;
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn emit_active_stack_begins(
    dst_start: u32,
    ptcl_capacity: u32,
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_kinds: &Array<u32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    ptcl_tags: &mut Array<u32>,
    ptcl_backdrops: &mut Array<i32>,
    ptcl_fill_rules: &mut Array<u32>,
    ptcl_segment_starts: &mut Array<u32>,
    ptcl_segment_ends: &mut Array<u32>,
    ptcl_colors: &mut Array<u32>,
) {
    let invalid = u32::new(-1);
    let mut stack_ix = layer_stack_start;
    let mut dst = dst_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        let layer_tag = layer_stack_tags[stack_i];
        if layer_tag == CUBE_LAYER_CLIP
            || layer_tag == CUBE_LAYER_OPACITY
            || layer_tag == CUBE_LAYER_BLEND
        {
            let draw_ix = layer_stack_draws[stack_i];
            if layer_tag == CUBE_LAYER_CLIP && draw_sdf_kinds[draw_ix as usize] != CUBE_SDF_NONE {
                store_particle(
                    dst,
                    ptcl_capacity,
                    u32::new(CUBE_PTCL_BEGIN_SDF_CLIP as i64),
                    0,
                    0,
                    0,
                    0,
                    draw_ix,
                    ptcl_tags,
                    ptcl_backdrops,
                    ptcl_fill_rules,
                    ptcl_segment_starts,
                    ptcl_segment_ends,
                    ptcl_colors,
                );
                dst += 1;
            } else {
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
                    let mut ptcl_tag = u32::new(CUBE_PTCL_BEGIN_CLIP as i64);
                    if layer_tag == CUBE_LAYER_OPACITY {
                        ptcl_tag = u32::new(CUBE_PTCL_BEGIN_OPACITY as i64);
                    } else if layer_tag == CUBE_LAYER_BLEND {
                        ptcl_tag = u32::new(CUBE_PTCL_BEGIN_BLEND as i64);
                    }
                    store_particle(
                        dst,
                        ptcl_capacity,
                        ptcl_tag,
                        backdrops[backdrop_i].load(),
                        draw_fill_rules[draw_ix as usize],
                        segment_starts[backdrop_i],
                        segment_ends[backdrop_i],
                        layer_stack_payloads[stack_i],
                        ptcl_tags,
                        ptcl_backdrops,
                        ptcl_fill_rules,
                        ptcl_segment_starts,
                        ptcl_segment_ends,
                        ptcl_colors,
                    );
                    dst += 1;
                }
            }
        }
        stack_ix += 1;
    }
}

#[cube]
fn emit_active_stack_ends(
    dst_start: u32,
    ptcl_capacity: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    layer_stack_tags: &Array<u32>,
    ptcl_tags: &mut Array<u32>,
    ptcl_backdrops: &mut Array<i32>,
    ptcl_fill_rules: &mut Array<u32>,
    ptcl_segment_starts: &mut Array<u32>,
    ptcl_segment_ends: &mut Array<u32>,
    ptcl_colors: &mut Array<u32>,
) {
    let mut stack_ix = layer_stack_end;
    let mut dst = dst_start;
    while stack_ix > layer_stack_start {
        stack_ix -= 1;
        let layer_tag = layer_stack_tags[stack_ix as usize];
        let mut ptcl_tag = u32::new(CUBE_PTCL_END_CLIP as i64);
        let mut valid = u32::new(1);
        if layer_tag == CUBE_LAYER_OPACITY {
            ptcl_tag = u32::new(CUBE_PTCL_END_OPACITY as i64);
        } else if layer_tag == CUBE_LAYER_BLEND {
            ptcl_tag = u32::new(CUBE_PTCL_END_BLEND as i64);
        } else if layer_tag != CUBE_LAYER_CLIP {
            valid = 0;
        }
        if valid == 1 {
            store_particle(
                dst,
                ptcl_capacity,
                ptcl_tag,
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
            dst += 1;
        }
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

    if path_id != invalid
        && (draw_tag == CUBE_DRAW_BRUSH
            || draw_tag == CUBE_DRAW_PATH_GLYPH
            || draw_tag == CUBE_DRAW_CLIP
            || draw_tag == CUBE_DRAW_OPACITY
            || draw_tag == CUBE_DRAW_BLEND
            || draw_tag == CUBE_DRAW_ISOLATE)
    {
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
fn draw_tile_hit(
    draw_i: usize,
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
) -> bool {
    let draw_x0 = pixel_tile_min(draw_pixel_x0[draw_i], tiles_width);
    let draw_y0 = pixel_tile_min(draw_pixel_y0[draw_i], tiles_height);
    let draw_x1 = pixel_tile_max(draw_pixel_x1[draw_i], tiles_width);
    let draw_y1 = pixel_tile_max(draw_pixel_y1[draw_i], tiles_height);
    tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn count_tile_glyphs_for_run(
    run_id: u32,
    tile_x: u32,
    tile_y: u32,
    glyph_run_starts: &Array<u32>,
    glyph_run_counts: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
) -> u32 {
    let mut count = 0u32;
    let mut glyph_ix = glyph_run_starts[run_id as usize];
    let glyph_end = glyph_ix + glyph_run_counts[run_id as usize];
    while glyph_ix < glyph_end {
        if glyph_hits_tile(
            glyph_ix,
            tile_x,
            tile_y,
            glyph_image_ids,
            glyph_x,
            glyph_y,
            glyph_image_left,
            glyph_image_top,
            glyph_image_width,
            glyph_image_height,
        ) {
            count += 1;
        }
        glyph_ix += 1;
    }
    count
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn store_tile_glyphs_for_run(
    dst_start: u32,
    glyph_capacity: u32,
    run_id: u32,
    tile_x: u32,
    tile_y: u32,
    glyph_run_starts: &Array<u32>,
    glyph_run_counts: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
    glyph_indices: &mut Array<u32>,
) {
    let mut count = 0u32;
    let mut glyph_ix = glyph_run_starts[run_id as usize];
    let glyph_end = glyph_ix + glyph_run_counts[run_id as usize];
    while glyph_ix < glyph_end {
        if glyph_hits_tile(
            glyph_ix,
            tile_x,
            tile_y,
            glyph_image_ids,
            glyph_x,
            glyph_y,
            glyph_image_left,
            glyph_image_top,
            glyph_image_width,
            glyph_image_height,
        ) {
            let dst = dst_start + count;
            if dst < glyph_capacity {
                glyph_indices[dst as usize] = glyph_ix;
            }
            count += 1;
        }
        glyph_ix += 1;
    }
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn glyph_hits_tile(
    glyph_ix: u32,
    tile_x: u32,
    tile_y: u32,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
) -> bool {
    let invalid = u32::new(-1);
    let glyph_i = glyph_ix as usize;
    let image_id = glyph_image_ids[glyph_i];
    let mut hit = false;
    if image_id != invalid {
        let image_i = image_id as usize;
        let width = glyph_image_width[image_i];
        let height = glyph_image_height[image_i];
        if width > 0 && height > 0 {
            let x0 = glyph_x[glyph_i] + glyph_image_left[image_i];
            let y0 = glyph_y[glyph_i] - glyph_image_top[image_i];
            let x1 = x0 + width as i32;
            let y1 = y0 + height as i32;
            let tile_x0 = (tile_x * 16) as i32;
            let tile_y0 = (tile_y * 16) as i32;
            let tile_x1 = tile_x0 + 16;
            let tile_y1 = tile_y0 + 16;
            hit = x0 < tile_x1 && x1 > tile_x0 && y0 < tile_y1 && y1 > tile_y0;
        }
    }
    hit
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
