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
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn staging_reuse_preserves_in_flight_and_resized_uploads() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::staging_reuse_preserves_in_flight_and_resized_uploads",
    )? {
        return Ok(());
    }
    let mut device = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let batch = |size, value| -> Result<_> {
        let mut batch = super::super::compute::ComputeBatch::new();
        for bytes in [vec![value; size], vec![value + 1; 128]] {
            let id = batch.buffer(bytes)?;
            batch.readback(id)?;
        }
        Ok(batch)
    };
    let handle = |device: &Dx12, ticket: &super::super::submissions::Ticket| {
        let super::work::Work::Compute(frame) = device.gpu.pending.get(ticket).unwrap() else {
            panic!("expected compute frame");
        };
        [
            frame.uploads[0].resource.as_raw(),
            frame.storage[0].resource.as_raw(),
        ]
    };
    let a = device.submit_compute(&batch(4096, 17)?)?;
    let b = device.submit_compute(&batch(4096, 29)?)?;
    let first = handle(&device, &a);
    let second = handle(&device, &b);
    assert_ne!(first[0], second[0]);
    assert_ne!(first[1], second[1]);
    assert!(device.gpu.staging.frames.is_empty());
    assert_eq!(
        device.readback_batch(&a)?,
        vec![vec![17; 4096], vec![18; 128]]
    );
    let c = device.submit_compute(&batch(2048, 53)?)?;
    assert_eq!(first, handle(&device, &c));
    assert_eq!(
        device.readback_batch(&c)?,
        vec![vec![53; 2048], vec![54; 128]]
    );
    assert_eq!(
        device.readback_batch(&b)?,
        vec![vec![29; 4096], vec![30; 128]]
    );
    // Resize retires all frame slots together. Both completed lists must survive
    // so rebuilding the pipeline does not allocate the second frame again.
    let d = device.submit_compute(&batch(4096, 41)?)?;
    let e = device.submit_compute(&batch(4096, 43)?)?;
    assert_eq!(second, handle(&device, &d));
    assert_eq!(first, handle(&device, &e));
    assert_eq!(
        device.readback_batch(&e)?,
        vec![vec![43; 4096], vec![44; 128]]
    );
    assert_eq!(
        device.readback_batch(&d)?,
        vec![vec![41; 4096], vec![42; 128]]
    );
    // Resource order changes between frames; small uploads must not consume
    // the large allocation and force the following large upload to allocate.
    let uploads: Vec<_> = device
        .gpu
        .staging
        .frames
        .last()
        .unwrap()
        .iter()
        .map(|buffer| buffer.resource.as_raw())
        .collect();
    let storage: Vec<_> = device
        .gpu
        .storage
        .frames
        .last()
        .unwrap()
        .iter()
        .map(|buffer| buffer.resource.as_raw())
        .collect();
    let mut reordered = super::super::compute::ComputeBatch::new();
    for bytes in [vec![61; 128], vec![63; 4096]] {
        let id = reordered.buffer(bytes)?;
        reordered.readback(id)?;
    }
    let ticket = device.submit_compute(&reordered)?;
    let super::work::Work::Compute(frame) = device.gpu.pending.get(&ticket).unwrap() else {
        panic!("expected compute frame");
    };
    assert_eq!(frame.uploads[0].resource.as_raw(), uploads[1]);
    assert_eq!(frame.uploads[1].resource.as_raw(), uploads[0]);
    assert_eq!(frame.storage[0].resource.as_raw(), storage[1]);
    assert_eq!(frame.storage[1].resource.as_raw(), storage[0]);
    assert_eq!(
        device.readback_batch(&ticket)?,
        vec![vec![61; 128], vec![63; 4096]]
    );
    for (size, value) in [(8192, 71), (128, 91), (16384, 113)] {
        let ticket = device.submit_compute(&batch(size, value)?)?;
        assert_eq!(
            device.readback_batch(&ticket)?,
            vec![vec![value; size], vec![value + 1; 128]]
        );
    }
    let cached = device.gpu.staging.frames.last().unwrap()[0]
        .resource
        .as_raw();
    let cached_storage = device.gpu.storage.frames.last().unwrap()[0]
        .resource
        .as_raw();
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    assert!(device.readback_batch(&empty)?.is_empty());
    assert_eq!(
        device
            .gpu
            .staging
            .frames
            .last()
            .unwrap()
            .first()
            .map(|buffer| buffer.resource.as_raw()),
        Some(cached)
    );
    assert_eq!(
        device
            .gpu
            .storage
            .frames
            .last()
            .unwrap()
            .first()
            .map(|buffer| buffer.resource.as_raw()),
        Some(cached_storage)
    );
    assert_valid(&device.validation_queue())?;
    // Execute succeeded, Signal did not: the new upload remains quarantined and
    // neither retirement nor another submit may recycle its storage.
    let free_uploads = device.gpu.staging.frames.len();
    let free_storage = device.gpu.storage.frames.len();
    device.inject_signal_failure = true;
    assert!(device.submit_compute(&batch(128, 127)?).is_err());
    assert!(device.unconfirmed());
    assert_eq!(device.pending_count(), 1);
    assert_eq!(device.gpu.staging.frames.len(), free_uploads - 1);
    assert_eq!(device.gpu.storage.frames.len(), free_storage - 1);
    assert!(device.submit_compute(&batch(128, 131)?).is_err());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn completed_unused_buffers_survive_alternating_frame_sizes() -> Result<()> {
    if super::super::isolation::run(
        "native::runtime::dx12::tests::completed_unused_buffers_survive_alternating_frame_sizes",
    )? {
        return Ok(());
    }
    let mut device = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let batch = |sizes: &[usize]| -> Result<_> {
        let mut batch = super::super::compute::ComputeBatch::new();
        for &size in sizes {
            let id = batch.buffer(vec![17; size])?;
            batch.readback(id)?;
        }
        Ok(batch)
    };
    let a = device.submit_compute(&batch(&[4096, 128])?)?;
    let super::work::Work::Compute(frame) = device.gpu.pending.get(&a).unwrap() else {
        panic!("expected compute frame");
    };
    let large_upload = frame.uploads[0].resource.as_raw();
    let large_storage = frame.storage[0].resource.as_raw();
    assert_eq!(
        device.readback_batch(&a)?,
        vec![vec![17; 4096], vec![17; 128]]
    );

    let b = device.submit_compute(&batch(&[128])?)?;
    assert_eq!(device.readback_batch(&b)?, vec![vec![17; 128]]);
    let empty = device.submit_compute(&super::super::compute::ComputeBatch::new())?;
    assert!(device.readback_batch(&empty)?.is_empty());
    let c = device.submit_compute(&batch(&[4096])?)?;
    let super::work::Work::Compute(frame) = device.gpu.pending.get(&c).unwrap() else {
        panic!("expected compute frame");
    };
    assert_eq!(frame.uploads[0].resource.as_raw(), large_upload);
    assert_eq!(frame.storage[0].resource.as_raw(), large_storage);
    assert_eq!(device.readback_batch(&c)?, vec![vec![17; 4096]]);
    Ok(())
}
