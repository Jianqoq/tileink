use super::*;

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
