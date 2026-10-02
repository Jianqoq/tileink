use super::*;
use crate::{
    render::fine::FineParams,
    shared::{gpu_constants::TILE_SIZE, gpu_plan::GpuBufferLengths},
};

#[test]
fn native_fine_rejects_target_and_spill_capacity_before_recording() -> Result<()> {
    let plan = FinePlan::new(
        GpuBufferLengths {
            tile_count: 1,
            tiles_width: 1,
            tiles_height: 1,
            ..Default::default()
        },
        FineParams {
            width: TILE_SIZE,
            height: TILE_SIZE,
            clip_spill_depth: 1,
            ..Default::default()
        },
        1,
    )?;
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let dummy = batch.buffer(vec![0; 4])?;
    let mut inputs = FineBindings {
        target,
        draws: dummy,
        paint: dummy,
        coarse: dummy,
        segments: dummy,
        text: dummy,
        spills: dummy,
        atlas: dummy,
        images: dummy,
        sampler: dummy,
    };
    // SAFETY: both cases fail the output-capacity preflight before any dispatch;
    // the remaining placeholders are never interpreted by the CPU or GPU.
    assert!(
        unsafe { encode(&mut batch, &plan, &inputs) }
            .unwrap_err()
            .to_string()
            .contains("target")
    );
    assert!(batch.passes().is_empty());
    inputs.target = batch.texture_rgba8(
        [TILE_SIZE, TILE_SIZE],
        vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize],
    )?;
    assert!(
        unsafe { encode(&mut batch, &plan, &inputs) }
            .unwrap_err()
            .to_string()
            .contains("spill")
    );
    assert!(batch.passes().is_empty());
    let mut other = ComputeBatch::new();
    assert!(unsafe { encode(&mut other, &plan, &inputs) }.is_err());
    Ok(())
}
