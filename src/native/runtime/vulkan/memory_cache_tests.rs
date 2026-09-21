use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn storage_buffers_are_reused_after_completion_only() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::vulkan::memory_cache_tests::storage_buffers_are_reused_after_completion_only",
    )? {
        return Ok(());
    }
    let mut device = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let messages = device.validation_messages();
    let batch = |size, value| -> Result<_> {
        let mut batch = super::super::compute::ComputeBatch::new();
        let id = batch.buffer(vec![value; size])?;
        batch.readback(id)?;
        Ok(batch)
    };
    let buffers = |device: &Vulkan, ticket: &super::super::submissions::Ticket| {
        let work::Work::Compute(frame) = device.pending.get(ticket).unwrap() else {
            unreachable!()
        };
        frame.storage_buffers().to_vec()
    };
    let a = device.submit_compute(&batch(4096, 17)?)?;
    let b = device.submit_compute(&batch(4096, 29)?)?;
    let first = buffers(&device, &a);
    let second = buffers(&device, &b);
    assert_ne!(first, second, "in-flight storage must not alias");
    assert_eq!(device.readback_batch(&a)?, vec![vec![17; 4096]]);
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    assert!(device.readback_batch(&empty)?.is_empty());
    let c = device.submit_compute(&batch(2048, 53)?)?;
    assert_eq!(
        buffers(&device, &c),
        first,
        "completed storage must be reused"
    );
    assert_eq!(device.readback_batch(&c)?, vec![vec![53; 2048]]);
    assert_eq!(device.readback_batch(&b)?, vec![vec![29; 4096]]);
    let d = device.submit_compute(&batch(1024, 71)?)?;
    let e = device.submit_compute(&batch(4096, 91)?)?;
    assert_eq!(buffers(&device, &d), second);
    assert_eq!(
        buffers(&device, &e),
        first,
        "all retired slots must survive a drain"
    );
    assert_eq!(device.readback_batch(&e)?, vec![vec![91; 4096]]);
    assert_eq!(device.readback_batch(&d)?, vec![vec![71; 1024]]);
    for (size, value) in [(8192, 101), (128, 111)] {
        let ticket = device.submit_compute(&batch(size, value)?)?;
        assert_eq!(device.readback_batch(&ticket)?, vec![vec![value; size]]);
    }
    device.injected_submit_error = Some(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
    assert!(device.submit_compute(&batch(128, 123)?).is_err());
    assert_eq!(device.pending_count(), 0);
    let retry = device.submit_compute(&batch(128, 131)?)?;
    assert_eq!(device.readback_batch(&retry)?, vec![vec![131; 128]]);
    drop(device);
    assert!(messages.lock().unwrap().is_empty());
    Ok(())
}
