//! Conformance checks exercise the production ComputeBatch, not an alternate
//! encoder/uniform-arena model that the renderer never submits.
use crate::native::runtime::{
    Result,
    adapter::Adapter,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::{
    NativeBackend, NativeContext, NativeContextOptions, shared::filter_config::FilterConfig,
};

fn context() -> Result<NativeContext> {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )
    .map_err(Into::into)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn compute_batch_submits_once_and_keeps_each_dispatch_uniform_immutable() -> Result<()> {
    let context = context()?;
    let mut batch = ComputeBatch::new();
    let colors = [0xff123456u32, 0x7f876543, 0x00112233];
    for color in colors {
        let image = batch.texture_rgba8([3, 2], vec![0; 24])?;
        filter::encode(
            &mut batch,
            BasicFilter::Clear,
            FilterConfig {
                width: 3,
                height: 2,
                region_width: 3,
                region_height: 2,
                clear_color: color,
                ..Default::default()
            },
            None,
            None,
            image,
        )?;
        batch.readback(image)?;
    }
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(context.adapter.pending_count(), 1);
    drop(batch);
    assert_eq!(
        receipt.readback()?,
        colors.map(|c| c.to_le_bytes().repeat(6))
    );
    assert_eq!(context.adapter.pending_count(), 0);
    receipt.assert_valid()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_compute_work_preserves_device_and_receipts_pin_the_context() -> Result<()> {
    use crate::native::runtime::adapter::SubmitError;
    let context = context()?;
    let other = self::context()?;
    let texture = context.create_texture(3, 2)?;
    let mut batch = ComputeBatch::new();
    let id = batch.import_texture(&texture)?;
    batch.readback(id)?;
    assert!(matches!(
        other.adapter.submit_compute(&batch),
        Err(SubmitError::Rejected(_))
    ));
    assert_eq!(context.adapter.pending_count(), 0);
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    assert!(other.adapter.readback(&receipt).is_err());
    let backend = context.backend();
    drop(batch);
    drop(texture);
    drop(context);
    let recreated = Adapter::with_options(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    assert!(recreated.readback(&receipt).is_err());
    assert_eq!(receipt.readback()?, vec![vec![0; 24]]);
    assert!(receipt.readback().is_err());
    receipt.assert_valid()?;
    other.check_validation()?;
    recreated.assert_valid()?;
    Ok(())
}
