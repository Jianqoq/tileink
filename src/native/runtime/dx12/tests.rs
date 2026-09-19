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
    for (state, released) in [
        (Retirement::Idle, 1),
        (Retirement::Unfenced, 0),
        (Retirement::Failed, 0),
    ] {
        let mut context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
        let observer: windows::core::IUnknown = context.gpu.fence.cast()?;
        let before = fence_references(&observer);
        let report = context.validation_queue();
        let message_count = unsafe {
            report
                .queue
                .as_ref()
                .expect("validation enabled")
                .GetNumStoredMessages()
        };
        context.retirement = state;
        drop(context);
        if released == 0 {
            assert!(
                unsafe {
                    report
                        .queue
                        .as_ref()
                        .expect("validation enabled")
                        .GetNumStoredMessages()
                } > message_count,
                "failed teardown must be observable"
            );
        }
        assert_eq!(fence_references(&observer), before - released);
    }
    Ok(())
}

fn fence_references(fence: &windows::core::IUnknown) -> u32 {
    unsafe {
        let count = (fence.vtable().AddRef)(fence.as_raw());
        (fence.vtable().Release)(fence.as_raw());
        count - 1
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn probe_pipelines_are_created_only_for_probe_submissions() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::probe_pipelines_are_created_only_for_probe_submissions",
    )? {
        return Ok(());
    }
    let mut device = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    assert!(device.gpu.signature.is_none());
    assert!(device.gpu.pipelines.is_empty());
    assert!(device.gpu.compute_pipelines.is_empty());
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
    let ticket = device.submit(&command)?;
    assert!(device.gpu.signature.is_some());
    assert!(!device.gpu.pipelines.is_empty());
    device.readback(&ticket)?;
    assert_valid(&device.validation_queue())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn late_validation_preserves_an_existing_plain_context() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::late_validation_preserves_an_existing_plain_context",
    )? {
        return Ok(());
    }
    use crate::{NativeBackend, NativeContext, NativeContextOptions, NativeRenderer};
    let options = NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: false,
    };
    let context = NativeContext::new(NativeBackend::Dx12, &options)?;
    let mut renderer = NativeRenderer::with_context(&context, 3, 2)?;
    let mut canvas = crate::Canvas::new(3, 2, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 3.0, 2.0),
        crate::Radius::ZERO,
        peniko::Color::from_rgb8(71, 133, 211),
    );
    let before = renderer.render_to_image(&canvas)?.readback()?.pixels;
    assert!(
        NativeContext::new(
            NativeBackend::Dx12,
            &NativeContextOptions {
                validation: true,
                ..options
            }
        )
        .is_err()
    );
    // No externally owned device exists in this isolated test. Tileink detects
    // its own already-created device and rejects the late process-wide request.
    assert!(unsafe { NativeContext::enable_dx12_validation() }.is_err());
    let after = renderer.render_to_image(&canvas)?.readback()?.pixels;
    assert_eq!(before, after);
    assert_eq!(context.adapter.pending_count(), 0);
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn disabled_validation_never_panics_before_failed_retirement_quarantine() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::disabled_validation_never_panics_before_failed_retirement_quarantine",
    )? {
        return Ok(());
    }
    let mut context = Dx12::with_options(&crate::native::NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: false,
    })?;
    let observer: windows::core::IUnknown = context.gpu.fence.cast()?;
    let before = fence_references(&observer);
    context.retirement = Retirement::Failed;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(context)));
    assert!(
        result.is_ok(),
        "missing debug queue must not panic during retirement"
    );
    assert_eq!(
        fence_references(&observer),
        before,
        "unknown-completion fence owner must remain pinned"
    );
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
        let start = report
            .queue
            .as_ref()
            .expect("validation enabled")
            .GetNumStoredMessages();
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .AddMessage(
                D3D12_MESSAGE_CATEGORY_STATE_CREATION,
                D3D12_MESSAGE_SEVERITY_ERROR,
                D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB,
                windows::core::s!("expected cache test diagnostic"),
            )?;
        report.record_cache_rejection(start)?;
        assert_valid(&report)?;
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .AddMessage(
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
    let before = unsafe {
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .GetNumStoredMessages()
    };
    drop(device);
    assert!(
        unsafe {
            report
                .queue
                .as_ref()
                .expect("validation enabled")
                .GetNumStoredMessages()
        } > before
    );
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
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .AddMessage(
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
            report
                .queue
                .as_ref()
                .expect("validation enabled")
                .GetNumStoredMessages(),
            1,
            "keep original messages"
        );
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .AddMessage(
                D3D12_MESSAGE_CATEGORY_EXECUTION,
                D3D12_MESSAGE_SEVERITY_ERROR,
                D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE,
                windows::core::s!("even this ID is fatal at error severity"),
            )?;
        assert!(super::assert_valid_with_wgpu_clears(&report).is_err());
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .ClearStoredMessages();
        report
            .queue
            .as_ref()
            .expect("validation enabled")
            .AddMessage(
                D3D12_MESSAGE_CATEGORY_EXECUTION,
                D3D12_MESSAGE_SEVERITY_WARNING,
                D3D12_MESSAGE_ID_UNKNOWN,
                windows::core::s!("unrelated correctness warning"),
            )?;
        assert!(super::assert_valid_with_wgpu_clears(&report).is_err());
    }
    Ok(())
}
