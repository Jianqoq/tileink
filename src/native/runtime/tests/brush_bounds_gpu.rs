use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, SamplerFilter},
};
use crate::shared::fine_config::FineConfig;
use crate::{NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_brush_reads_preserve_valid_words_and_reject_out_of_range_addresses() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: this single-threaded GPU test enables validation before creating devices.
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
    let table_len = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|shader| shader.entry == "brush_words")
        .unwrap()
        .bindings
        .iter()
        .find(|binding| binding.slot == 30)
        .unwrap()
        .count as usize;
    let color = u32::from_le_bytes([71, 19, 23, 255]);
    // The color is the final valid word. Invalid requests exercise the end of
    // the descriptor, a nonzero brush base, and addition-overflow candidates.
    for (prefix, base) in [(0, 0), (7, 7), (7, 12), (7, 13), (7, u32::MAX)] {
        let mut batch = ComputeBatch::new();
        let config = batch.buffer(
            bytemuck::bytes_of(&FineConfig {
                paint_brush_base: base,
                ..Default::default()
            })
            .to_vec(),
        )?;
        let mut words = vec![0u32; prefix];
        words.extend([1, 0, 0, 0, color]);
        let paint = batch.buffer(bytemuck::cast_slice(&words).to_vec())?;
        let requests: Vec<u32> = [0, 5, 12, u32::MAX - 8]
            .into_iter()
            .flat_map(|offset| [offset, 0, 0, 0])
            .collect();
        let requests = batch.buffer(bytemuck::cast_slice(&requests).to_vec())?;
        let request_config = batch.buffer(bytemuck::cast_slice(&[4u32, 0, 0, 0]).to_vec())?;
        let output = batch.buffer(vec![0xa5; 32])?;
        let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
        let texture = batch.texture_rgba8([1, 1], vec![0; 4])?;
        let images = batch.texture_table(&vec![texture; table_len])?;
        let sampler = batch.sampler(SamplerFilter::Nearest)?;
        // SAFETY: brush_words guards every paint read; invalid bases/offsets are
        // deliberate inputs to that contract. Four requests own four output words.
        unsafe {
            batch.dispatch(
                "brush_words",
                &[
                    (0, config),
                    (3, paint),
                    (9, requests),
                    (10, output),
                    (11, request_config),
                    (12, atlas),
                    (13, sampler),
                    (30, images),
                ],
                [1, 1, 1],
            )?;
        }
        batch.readback(output)?;
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        let first = if base == prefix as u32 { color } else { 0 };
        let expected = [
            first, 0, 0, 0, 0xa5a5a5a5, 0xa5a5a5a5, 0xa5a5a5a5, 0xa5a5a5a5,
        ];
        assert_eq!(
            receipt.readback()?,
            vec![bytemuck::cast_slice::<u32, u8>(&expected).to_vec()],
            "base {base}"
        );
    }
    context.check_validation()?;
    Ok(())
}
