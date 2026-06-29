use ::cubecl::prelude::*;

use crate::cubecl::{
    renderer::{ScanBuffers, SceneBuffers},
    types::{CUMSUM_CHUNK_SIZE, CubeBufferLengths},
};

const WORKGROUP_SIZE: u32 = 256;

pub(crate) struct CumsumPipeline;

impl CumsumPipeline {
    pub(crate) fn run<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &mut ScanBuffers,
        lengths: CubeBufferLengths,
    ) {
        let chunk_count = lengths.cumsum_chunk_count as u32;
        if chunk_count == 0 {
            return;
        }

        cumsum_prefix_chunks::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(CUMSUM_CHUNK_SIZE),
            CUMSUM_CHUNK_SIZE as usize,
            unsafe { scene.cumsum_chunk_backdrop_offsets.arg() },
            unsafe { scene.cumsum_chunk_lens.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.cumsum_chunk_totals.arg() },
        );

        let row_count = lengths.cumsum_row_count as u32;
        // If every backdrop row fits in one chunk, the per-chunk prefix above is
        // already the full row prefix. There is no inter-chunk carry to compute.
        if row_count == chunk_count {
            return;
        }

        if row_count > 0 {
            cumsum_chunk_offsets::launch::<R>(
                client,
                cube_count(row_count),
                CubeDim::new_1d(WORKGROUP_SIZE),
                row_count,
                unsafe { scene.cumsum_row_chunk_starts.arg() },
                unsafe { scene.cumsum_row_chunk_ends.arg() },
                unsafe { scan.cumsum_chunk_totals.arg() },
                unsafe { scan.cumsum_chunk_offsets.arg() },
            );
        }

        cumsum_apply_chunk_offsets::launch::<R>(
            client,
            CubeCount::Static(chunk_count, 1, 1),
            CubeDim::new_1d(CUMSUM_CHUNK_SIZE),
            unsafe { scene.cumsum_chunk_backdrop_offsets.arg() },
            unsafe { scene.cumsum_chunk_lens.arg() },
            unsafe { scan.cumsum_chunk_offsets.arg() },
            unsafe { scan.backdrops.arg() },
        );
    }
}

fn cube_count(items: u32) -> CubeCount {
    CubeCount::Static(items.div_ceil(WORKGROUP_SIZE), 1, 1)
}

#[cube(launch)]
fn cumsum_prefix_chunks(
    #[comptime] chunk_size: usize,
    chunk_backdrop_offsets: &Array<u32>,
    chunk_lens: &Array<u32>,
    backdrops: &mut Array<Atomic<i32>>,
    chunk_totals: &mut Array<i32>,
) {
    let chunk_ix = CUBE_POS;
    let lane = UNIT_POS as usize;
    let chunk_offset = chunk_backdrop_offsets[chunk_ix];
    let chunk_len = chunk_lens[chunk_ix];
    let mut value = i32::new(0);
    if lane < chunk_len as usize {
        value = backdrops[(chunk_offset + lane as u32) as usize].load();
    }
    let mut shared = SharedMemory::<i32>::new(chunk_size);
    shared[lane] = value;
    sync_cube();

    // Keep the loop counter runtime-visible: a plain mutable scalar is folded
    // as comptime by CubeCL 0.10 and cannot be updated inside the kernel.
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
        chunk_totals[chunk_ix] = shared[chunk_size - 1];
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
        backdrops[(chunk_offset + lane as u32) as usize].store(shared[lane] + value);
    }
}

#[cube(launch)]
fn cumsum_chunk_offsets(
    row_count: u32,
    row_chunk_starts: &Array<u32>,
    row_chunk_ends: &Array<u32>,
    chunk_totals: &Array<i32>,
    chunk_offsets: &mut Array<i32>,
) {
    let row_ix = ABSOLUTE_POS as u32;
    if row_ix >= row_count {
        terminate!();
    }

    let row_i = row_ix as usize;
    let mut carry = i32::new(0);
    let mut chunk_ix = row_chunk_starts[row_i];
    let chunk_end = row_chunk_ends[row_i];
    while chunk_ix < chunk_end {
        let ix = chunk_ix as usize;
        chunk_offsets[ix] = carry;
        carry += chunk_totals[ix];
        chunk_ix += 1;
    }
}

#[cube(launch)]
fn cumsum_apply_chunk_offsets(
    chunk_backdrop_offsets: &Array<u32>,
    chunk_lens: &Array<u32>,
    chunk_offsets: &Array<i32>,
    backdrops: &mut Array<Atomic<i32>>,
) {
    let chunk_ix = CUBE_POS;
    let lane = UNIT_POS;
    let chunk_len = chunk_lens[chunk_ix];
    if lane >= chunk_len {
        terminate!();
    }

    let ix = (chunk_backdrop_offsets[chunk_ix] + lane) as usize;
    backdrops[ix].store(backdrops[ix].load() + chunk_offsets[chunk_ix]);
}
