use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn probe_pipelines_are_lazy_and_partial_initialization_can_retry() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::vulkan::tests::probe_pipelines_are_lazy_and_partial_initialization_can_retry",
    )? {
        return Ok(());
    }
    let mut device = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    assert_eq!(device.layout, vk::PipelineLayout::null());
    assert!(device.pipelines.is_empty());
    assert!(device.compute_pipelines.is_empty());
    let command = super::super::program::Probe {
        entry: "clear_words",
        params: super::super::program::Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0x12345678, 0, 0, 0],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    };
    device.inject_probe_init_failure = true;
    assert!(device.submit(&command).is_err());
    assert!(device.pipelines.is_empty());
    assert_eq!(device.layout, vk::PipelineLayout::null());
    assert_eq!(device.bindings, vk::DescriptorSetLayout::null());
    assert_eq!(device.pending_count(), 0);
    let ticket = device.submit(&command)?;
    assert!(!device.pipelines.is_empty());
    device.readback(&ticket)?;
    assert!(device.validation_messages().lock().unwrap().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn failed_teardown_is_reported() -> Result<()> {
    if super::super::isolation::run("native::runtime::vulkan::tests::failed_teardown_is_reported")?
    {
        return Ok(());
    }
    let mut context = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let messages = context.validation_messages();
    context.failed = true;
    drop(context);
    assert!(
        messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("cleanup could not confirm completion"))
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn submit_errors_preserve_prefix_and_classify_rejected_vs_unconfirmed() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::vulkan::tests::submit_errors_preserve_prefix_and_classify_rejected_vs_unconfirmed",
    )? {
        return Ok(());
    }
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let command = super::super::program::Probe {
        entry: "clear_words",
        params: super::super::program::Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0x12345678, 0, 0, 0],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    };
    for error in [
        vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        vk::Result::ERROR_OUT_OF_DEVICE_MEMORY,
        vk::Result::ERROR_DEVICE_LOST,
        vk::Result::ERROR_UNKNOWN,
    ] {
        let mut device = Vulkan::new(&identity)?;
        let messages = device.validation_messages();
        let prefix = device.submit(&command)?;
        // Inject the API result at the queue-call boundary after allocation and
        // lease registration; do not exhaust the machine's real GPU memory.
        device.injected_submit_error = Some(error);
        assert!(device.submit(&command).is_err());
        let rejected = matches!(
            error,
            vk::Result::ERROR_OUT_OF_HOST_MEMORY | vk::Result::ERROR_OUT_OF_DEVICE_MEMORY
        );
        assert_eq!(device.unconfirmed(), !rejected);
        assert_eq!(device.pending_count(), if rejected { 1 } else { 2 });
        if rejected {
            let retry = device.submit(&command)?;
            assert_eq!(device.readback(&retry)?, 0x12345678u32.to_le_bytes());
            assert_eq!(device.readback(&prefix)?, 0x12345678u32.to_le_bytes());
        } else {
            assert!(device.submit(&command).is_err());
            assert!(device.readback(&prefix).is_err());
        }
        drop(device);
        assert_eq!(messages.lock().unwrap().is_empty(), rejected);
    }
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn staging_reuse_preserves_in_flight_and_resized_uploads() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::vulkan::tests::staging_reuse_preserves_in_flight_and_resized_uploads",
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
    let handle = |device: &Vulkan, ticket: &super::super::submissions::Ticket| {
        let super::work::Work::Compute(frame) = device.pending.get(ticket).unwrap() else {
            panic!("expected compute frame");
        };
        frame.upload.as_ref().unwrap().arena.buffers[0]
    };
    let a = device.submit_compute(&batch(4096, 17)?)?;
    let b = device.submit_compute(&batch(4096, 29)?)?;
    let first = handle(&device, &a);
    assert_ne!(first, handle(&device, &b));
    assert!(device.frame_cache.staging.is_none());
    assert_eq!(device.readback_batch(&a)?, vec![vec![17; 4096]]);
    let c = device.submit_compute(&batch(2048, 53)?)?;
    assert_eq!(first, handle(&device, &c));
    assert_eq!(device.readback_batch(&c)?, vec![vec![53; 2048]]);
    assert_eq!(device.readback_batch(&b)?, vec![vec![29; 4096]]);
    for (size, value) in [(8192, 71), (128, 91), (16384, 113)] {
        let ticket = device.submit_compute(&batch(size, value)?)?;
        assert_eq!(device.readback_batch(&ticket)?, vec![vec![value; size]]);
    }
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    assert!(device.readback_batch(&empty)?.is_empty());
    assert!(device.frame_cache.staging.is_some());
    device.injected_submit_error = Some(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
    assert!(device.submit_compute(&batch(128, 127)?).is_err());
    assert!(device.frame_cache.staging.is_none());
    assert_eq!(device.pending_count(), 0);
    let retry = device.submit_compute(&batch(128, 131)?)?;
    assert_eq!(device.readback_batch(&retry)?, vec![vec![131; 128]]);
    drop(device);
    assert!(messages.lock().unwrap().is_empty());
    Ok(())
}
