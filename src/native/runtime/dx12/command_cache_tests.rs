use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn completed_descriptor_heaps_are_reused_without_in_flight_aliasing() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::command_cache_tests::completed_descriptor_heaps_are_reused_without_in_flight_aliasing",
    )? {
        return Ok(());
    }
    let mut device = Dx12::with_options(&crate::NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: false,
    })?;
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
    let heaps = |device: &Dx12, ticket: &super::super::submissions::Ticket| {
        let work::Work::Compute(frame) = device.gpu.pending.get(ticket).unwrap() else {
            unreachable!()
        };
        frame.descriptor_heaps()
    };
    let first = device.submit_compute(&batch(4, 0xff112233)?)?;
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    let second = device.submit_compute(&batch(4, 0xff445566)?)?;
    let a = heaps(&device, &first);
    let b = heaps(&device, &second);
    let work::Work::Compute(frame) = device.gpu.pending.get(&first).unwrap() else {
        unreachable!()
    };
    let first_list = frame.list.clone();
    let work::Work::Compute(frame) = device.gpu.pending.get(&empty).unwrap() else {
        unreachable!()
    };
    let empty_list = frame.list.clone();
    assert_ne!(
        a[0].as_raw(),
        b[0].as_raw(),
        "in-flight heaps must remain exclusive"
    );
    assert_eq!(
        device.readback_batch(&first)?[0],
        0xff112233u32.to_le_bytes().repeat(64)
    );
    assert!(device.readback_batch(&empty)?.is_empty());
    let third = device.submit_compute(&batch(2, 0xff778899)?)?;
    let work::Work::Compute(frame) = device.gpu.pending.get(&third).unwrap() else {
        unreachable!()
    };
    assert!(
        [first_list.as_raw(), empty_list.as_raw()].contains(&frame.list.as_raw()),
        "completed command list should be reused"
    );
    assert_eq!(
        heaps(&device, &third)[0].as_raw(),
        a[0].as_raw(),
        "completed heap should be reused"
    );
    assert_eq!(
        device.readback_batch(&third)?[0],
        0xff778899u32.to_le_bytes().repeat(64)
    );
    assert_eq!(
        device.readback_batch(&second)?[0],
        0xff445566u32.to_le_bytes().repeat(64)
    );
    let large = device.submit_compute(&batch(40, 0xffaabbcc)?)?;
    assert_eq!(
        device.readback_batch(&large)?[0],
        0xffaabbccu32.to_le_bytes().repeat(64)
    );
    let free = device.gpu.tables.len();
    let free_commands = device.gpu.commands.len();
    device.inject_signal_failure = true;
    assert!(device.submit_compute(&batch(2, 0xff123456)?).is_err());
    assert!(device.unconfirmed());
    assert_eq!(device.gpu.commands.len(), free_commands - 1);
    assert_eq!(
        device.gpu.tables.len(),
        free - 1,
        "unknown completion cannot recycle heaps"
    );
    assert!(device.submit_compute(&batch(2, 0xff123456)?).is_err());
    assert_eq!(device.gpu.tables.len(), free - 1);
    Ok(())
}
