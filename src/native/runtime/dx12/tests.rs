use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn failed_retirement_retains_fence_owner_until_process_exit() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::failed_retirement_retains_fence_owner_until_process_exit",
    )? {
        return Ok(());
    }
    // COM reference counts are used only in this isolated lifetime regression.
    // The observer keeps querying safe; comparing before/after detects whether
    // the cleanup path released its own fence reference despite that observer.
    fn references(fence: &windows::core::IUnknown) -> u32 {
        unsafe {
            let count = (fence.vtable().AddRef)(fence.as_raw());
            (fence.vtable().Release)(fence.as_raw());
            count - 1
        }
    }
    for (state, released) in [
        (Retirement::Idle, 1),
        (Retirement::Unfenced, 0),
        (Retirement::Failed, 0),
    ] {
        let mut context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
        let observer: windows::core::IUnknown = context.gpu.fence.cast()?;
        let before = references(&observer);
        let report = context.validation_queue();
        let message_count = unsafe { report.queue.GetNumStoredMessages() };
        context.retirement = state;
        drop(context);
        if released == 0 {
            assert!(
                unsafe { report.queue.GetNumStoredMessages() } > message_count,
                "failed teardown must be observable"
            );
        }
        assert_eq!(references(&observer), before - released);
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_cache_diagnostics_do_not_hide_other_attempts() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::rejected_cache_diagnostics_do_not_hide_other_attempts",
    )? {
        return Ok(());
    }
    let context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let report = context.validation_queue();
    unsafe {
        let start = report.queue.GetNumStoredMessages();
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_STATE_CREATION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB,
            windows::core::s!("expected cache test diagnostic"),
        )?;
        report.record_cache_rejection(start)?;
        assert_valid(&report)?;
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_STATE_CREATION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB,
            windows::core::s!("outside rejected attempt"),
        )?;
        assert!(assert_valid(&report).is_err());
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn signal_failure_after_execute_retains_the_attempt_and_blocks_reuse() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::signal_failure_after_execute_retains_the_attempt_and_blocks_reuse",
    )? {
        return Ok(());
    }
    let mut device = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
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
    let prefix = device.submit(&command)?;
    device.inject_signal_failure = true;
    assert!(device.submit(&command).is_err());
    assert!(device.unconfirmed());
    assert_eq!(device.pending_count(), 2);
    assert!(device.readback(&prefix).is_err());
    assert!(device.submit(&command).is_err());
    let report = device.validation_queue();
    let before = unsafe { report.queue.GetNumStoredMessages() };
    drop(device);
    assert!(unsafe { report.queue.GetNumStoredMessages() } > before);
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn wgpu_clear_advisory_never_hides_correctness_errors() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::wgpu_clear_advisory_never_hides_correctness_errors",
    )? {
        return Ok(());
    }
    let context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let report = context.validation_queue();
    unsafe {
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_EXECUTION,
            D3D12_MESSAGE_SEVERITY_WARNING,
            D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE,
            windows::core::s!("reference optimized-clear advisory"),
        )?;
        assert!(
            assert_valid(&report).is_err(),
            "ordinary validation stays strict"
        );
        super::assert_valid_with_wgpu_clears(&report)?;
        assert_eq!(
            report.queue.GetNumStoredMessages(),
            1,
            "keep original messages"
        );
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_EXECUTION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE,
            windows::core::s!("even this ID is fatal at error severity"),
        )?;
        assert!(super::assert_valid_with_wgpu_clears(&report).is_err());
        report.queue.ClearStoredMessages();
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_EXECUTION,
            D3D12_MESSAGE_SEVERITY_WARNING,
            D3D12_MESSAGE_ID_UNKNOWN,
            windows::core::s!("unrelated correctness warning"),
        )?;
        assert!(super::assert_valid_with_wgpu_clears(&report).is_err());
    }
    Ok(())
}
