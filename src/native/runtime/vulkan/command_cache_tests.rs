use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn completed_command_pools_and_fences_are_reused() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::vulkan::command_cache_tests::completed_command_pools_and_fences_are_reused",
    )? {
        return Ok(());
    }
    let mut device = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let messages = device.validation_messages();
    let batch = |passes, color| -> Result<_> {
        use crate::native::runtime::program::filter::{self, BasicFilter};
        use crate::shared::filter_config::FilterConfig;
        let mut batch = super::super::compute::ComputeBatch::new();
        let image = batch.texture_rgba8([8, 8], vec![0; 256])?;
        for _ in 0..passes {
            filter::encode(
                &mut batch,
                BasicFilter::Clear,
                FilterConfig {
                    width: 8,
                    height: 8,
                    region_width: 8,
                    region_height: 8,
                    clear_color: color,
                    ..Default::default()
                },
                None,
                None,
                image,
            )?;
        }
        batch.readback(image)?;
        Ok(batch)
    };
    let handles = |device: &Vulkan, ticket: &super::super::submissions::Ticket| {
        let work::Work::Compute(frame) = device.pending.get(ticket).unwrap() else {
            unreachable!()
        };
        frame.command_handles()
    };
    let a = device.submit_compute(&batch(4, 0xff112233)?)?;
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    let b = device.submit_compute(&batch(4, 0xff445566)?)?;
    let first = handles(&device, &a);
    assert_ne!(first, handles(&device, &b));
    assert_eq!(
        device.readback_batch(&a)?[0],
        0xff112233u32.to_le_bytes().repeat(64)
    );
    assert!(device.readback_batch(&empty)?.is_empty());
    let c = device.submit_compute(&batch(2, 0xff778899)?)?;
    assert_eq!(
        handles(&device, &c),
        first,
        "completed command resources must be reused"
    );
    assert_eq!(
        device.readback_batch(&c)?[0],
        0xff778899u32.to_le_bytes().repeat(64)
    );
    assert_eq!(
        device.readback_batch(&b)?[0],
        0xff445566u32.to_le_bytes().repeat(64)
    );
    let large = device.submit_compute(&batch(40, 0xffaabbcc)?)?;
    // Isolate one completed slot so alternating descriptor classes test growth,
    // rather than accidentally selecting a different free pool.
    device.frame_cache.commands.clear();
    assert_eq!(
        device.readback_batch(&large)?[0],
        0xffaabbccu32.to_le_bytes().repeat(64)
    );
    let mut color = super::super::compute::ComputeBatch::new();
    let source = color.texture_rgba8([8, 8], 0xffaabbccu32.to_le_bytes().repeat(64))?;
    let target = color.texture_rgba8([8, 8], vec![0; 256])?;
    super::super::program::filter::encode(
        &mut color,
        super::super::program::filter::BasicFilter::Color,
        crate::shared::filter_config::FilterConfig {
            width: 8,
            height: 8,
            region_width: 8,
            region_height: 8,
            amount: 1.0,
            ..Default::default()
        },
        None,
        Some(source),
        target,
    )?;
    let sampled = device.submit_compute(&color)?;
    let grown = handles(&device, &sampled);
    device.readback_batch(&sampled)?;
    let many = device.submit_compute(&batch(40, 0xffaabbcc)?)?;
    assert_eq!(
        handles(&device, &many),
        grown,
        "growing sampled descriptors must preserve prior set/storage capacity"
    );
    device.readback_batch(&many)?;
    assert!(messages.lock().unwrap().is_empty());
    let free = device.frame_cache.commands.len();
    device.injected_submit_error = Some(vk::Result::ERROR_DEVICE_LOST);
    assert!(device.submit_compute(&batch(2, 0xff123456)?).is_err());
    assert!(device.failed);
    assert_eq!(
        device.frame_cache.commands.len(),
        free - 1,
        "unknown completion cannot recycle command resources"
    );
    assert!(device.submit_compute(&batch(2, 0xff123456)?).is_err());
    assert_eq!(device.frame_cache.commands.len(), free - 1);
    drop(device);
    Ok(())
}
