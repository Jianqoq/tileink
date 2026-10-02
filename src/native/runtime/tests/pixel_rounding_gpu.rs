use super::Result;
use crate::native::runtime::compute::ComputeBatch;
use crate::{NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_interpolation_preserves_fused_half_channel_rounding() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: single-threaded GPU tests enable validation before creating devices.
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
    // Separate multiply/add rounds these values onto a half-channel boundary;
    // fused interpolation remains below it. Exercise the production helper.
    let cases = [
        (6u32, 0u32, 0x3f6aaaabu32, 0u32),
        (7, 1, 0x3f6aaaab, 1),
        (10, 0, 0x3f59999a, 1),
        (11, 0, 0x3f5d1746, 1),
    ];
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytemuck::cast_slice(&[cases.len() as u32, 0, 0, 0]).to_vec())?;
    let words: Vec<u32> = cases
        .iter()
        .flat_map(|&(a, b, t, _)| [0, 0, a * 0x01010101, b * 0x01010101, 0, t, 0, 0])
        .collect();
    let source = batch.buffer(bytemuck::cast_slice(&words).to_vec())?;
    let output = batch.buffer(vec![0xa5; cases.len() * 56 + 4])?;
    // SAFETY: every 32-byte request has a complete 56-byte output record.
    unsafe {
        batch.dispatch(
            "pixel_math_words",
            &[(0, config), (1, source), (2, output)],
            [1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|error| format!("{error:?}"))?;
    let results = receipt.readback()?;
    for (index, &(_, _, _, expected)) in cases.iter().enumerate() {
        assert_eq!(
            &results[0][index * 56 + 28..index * 56 + 32],
            &(expected * 0x01010101).to_le_bytes()
        );
    }
    assert_eq!(&results[0][cases.len() * 56..], &[0xa5; 4]);
    context.check_validation()?;
    Ok(())
}
