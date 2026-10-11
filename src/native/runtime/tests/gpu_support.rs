use super::Result;
use crate::native::runtime::compute::{ComputeBatch, SamplerFilter};
use crate::shared::fine_config::FineConfig;
use crate::{NativeContext, NativeContextOptions};

pub(super) fn batch(context: &NativeContext) -> ComputeBatch {
    use crate::native::runtime::compute::SurfacePool;
    use std::{cell::RefCell, rc::Rc};
    ComputeBatch::with_surfaces(Rc::new(RefCell::new(SurfacePool::new(context))))
}

/// Test device identity is mandatory; a missing pin must never select another GPU.
pub(super) fn options() -> Result<NativeContextOptions> {
    Ok(NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: true,
    })
}

pub(super) fn context() -> Result<NativeContext> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    #[cfg(feature = "dx12")]
    let backend = crate::NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = crate::NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = crate::NativeBackend::Metal;
    Ok(NativeContext::new(backend, &options()?)?)
}

pub(super) fn sample(
    context: &NativeContext,
    words: &[u32],
    points: &[[f32; 2]],
) -> Result<Vec<u32>> {
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        bytemuck::bytes_of(&FineConfig {
            paint_brush_base: 5,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let mut paint = vec![0; 5];
    paint.extend_from_slice(words);
    let paint = batch.buffer(bytemuck::cast_slice(&paint).to_vec())?;
    let requests: Vec<_> = points
        .iter()
        .flat_map(|p| [7, p[0].to_bits(), p[1].to_bits(), 0])
        .collect();
    let requests = batch.buffer(bytemuck::cast_slice(&requests).to_vec())?;
    let request_config =
        batch.buffer(bytemuck::cast_slice(&[points.len() as u32, 0, 0, 0]).to_vec())?;
    let output = batch.buffer(vec![0xa5; points.len() * 4])?;
    let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
    let texture = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let table_len = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|s| s.entry == "brush_words")
        .unwrap()
        .bindings
        .iter()
        .find(|b| b.slot == 30)
        .unwrap()
        .count as usize;
    let images = batch.texture_table(&vec![texture; table_len])?;
    let sampler = batch.sampler(SamplerFilter::Nearest);
    // SAFETY: each request owns one output word; all images and bounded paint
    // storage remain owned by this batch. Corrupt records test robust rejection.
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
            [points.len().div_ceil(256) as u32, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    Ok(receipt.readback()?[0]
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect())
}
