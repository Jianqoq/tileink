use super::super::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::{NativeContext, NativeContextOptions, TILE_SIZE, shared::filter_config::FilterConfig};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_cached_filter_tiles_preserve_sparse_history_across_ordered_passes() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: serial GPU tests enable the layer before any device creation.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let size = [273u32, 305];
    let columns = size[0].div_ceil(TILE_SIZE);
    let count = columns * size[1].div_ceil(TILE_SIZE);
    let active: Vec<u32> = (0..count).rev().filter(|t| t % 5 != 2).collect();
    let changed: Vec<u32> = active.iter().copied().step_by(4).collect();
    let original_a = [11u8, 23, 41, 255];
    let original_b = [61u8, 19, 7, 255];
    let color = [31u8, 57, 111, 255];
    let changed_color = [101u8, 33, 75, 255];
    let pixels = (size[0] * size[1]) as usize;
    let mut batch = ComputeBatch::new();
    let a = batch.texture_rgba8(size, original_a.repeat(pixels))?;
    let b = batch.texture_rgba8(size, original_b.repeat(pixels))?;
    let config = FilterConfig {
        width: size[0],
        height: size[1],
        region_width: size[0],
        region_height: size[1],
        clear_color: u32::from_le_bytes(color),
        ..Default::default()
    };
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        config,
        Some(&active),
        None,
        a,
    )?;
    filter::encode(
        &mut batch,
        BasicFilter::Copy,
        config,
        Some(&active.clone()),
        Some(a),
        b,
    )?;
    let binding = |index: usize| {
        batch.passes()[index]
            .bindings
            .iter()
            .find(|(binding, _)| binding.slot == 8)
            .unwrap()
            .1
    };
    assert_eq!(
        binding(0),
        binding(1),
        "two stages must share an immutable tile buffer"
    );
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        FilterConfig {
            clear_color: u32::from_le_bytes(changed_color),
            ..config
        },
        Some(&changed),
        None,
        a,
    )?;
    batch.readback(a)?;
    batch.readback(b)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    drop(batch);
    let mut selected = vec![false; count as usize];
    let mut altered = selected.clone();
    for tile in active {
        selected[tile as usize] = true;
    }
    for tile in changed {
        altered[tile as usize] = true;
    }
    let mut expected_a = Vec::with_capacity(pixels * 4);
    let mut expected_b = Vec::with_capacity(pixels * 4);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let tile = (y / TILE_SIZE * columns + x / TILE_SIZE) as usize;
            expected_a.extend(if altered[tile] {
                changed_color
            } else if selected[tile] {
                color
            } else {
                original_a
            });
            expected_b.extend(if selected[tile] { color } else { original_b });
        }
    }
    assert_eq!(receipt.readback()?, [expected_a, expected_b]);
    context.check_validation()?;
    Ok(())
}
