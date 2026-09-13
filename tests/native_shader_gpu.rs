#![cfg(windows)]
#[path = "native_shader_gpu/cases.rs"]
mod cases;
#[path = "native_shader_gpu/dx12.rs"]
mod dx12;
#[path = "../examples/common/gpu_identity.rs"]
mod gpu_identity;
#[path = "native_shader_gpu/pipeline_cache.rs"]
mod pipeline_cache;
#[path = "native_shader_gpu/wgpu.rs"]
mod reference;
#[path = "native_shader_gpu/retirement.rs"]
mod retirement;
#[path = "native_shader_gpu/submissions.rs"]
mod submissions;
#[path = "native_shader_gpu/vulkan.rs"]
mod vulkan;

#[path = "native_shader_gpu/isolation.rs"]
mod isolation;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_probes_match_independent_cpu_results() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let mut vulkan = vulkan::Vulkan::new(&identity)?;
    let mut dx12 = dx12::Dx12::new(&identity)?;
    let wgpu_dx12 = reference::Reference::new(wgpu::Backends::DX12, &identity)?;
    let wgpu_vulkan = reference::Reference::new(wgpu::Backends::VULKAN, &identity)?;
    let vulkan_messages = vulkan.validation_messages();
    let dx12_messages = dx12.validation_queue();
    for case in cases::cases() {
        let actual = vulkan.execute(&case)?;
        let directx = dx12.execute(&case)?;
        let reference_dx12 = wgpu_dx12.execute(&case)?;
        let reference_vulkan = wgpu_vulkan.execute(&case)?;
        assert_eq!(reference_dx12, directx, "wgpu DX12/native DX12");
        assert_eq!(reference_vulkan, directx, "wgpu Vulkan/native DX12");
        assert_eq!(directx, case.expected, "native DX12");
        assert_eq!(directx, actual, "native DX12/Vulkan equality");
        assert_eq!(
            actual, case.expected,
            "native Vulkan {}/{}",
            case.entry, case.params.count
        );
    }
    drop(dx12);
    drop(vulkan);
    dx12::assert_valid(&dx12_messages)?;
    let messages = vulkan_messages.lock().unwrap();
    assert!(messages.is_empty(), "Vulkan validation: {messages:?}");
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn queued_native_submissions_keep_leases_and_reject_wrong_devices() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let mut dx12 = dx12::Dx12::new(&identity)?;
    let mut vulkan = vulkan::Vulkan::new(&identity)?;
    let dx12_messages = dx12.validation_queue();
    let vulkan_messages = vulkan.validation_messages();
    let cases = cases::cases();
    let mut tickets = Vec::new();
    for case in &cases {
        tickets.push((dx12.submit(case)?, vulkan.submit(case)?));
    }
    assert_eq!(dx12.pending_count(), cases.len());
    assert_eq!(vulkan.pending_count(), cases.len());
    let wrong = dx12.readback(&tickets[0].1).unwrap_err();
    assert_eq!(
        wrong.downcast_ref::<submissions::SubmissionError>(),
        Some(&submissions::SubmissionError::WrongDevice)
    );
    let wrong = vulkan.readback(&tickets[0].0).unwrap_err();
    assert_eq!(
        wrong.downcast_ref::<submissions::SubmissionError>(),
        Some(&submissions::SubmissionError::WrongDevice)
    );
    // Read the newest frame first: waiting for it must not overwrite or discard
    // earlier unread outputs. Each submitted case owns distinct staging state.
    for (case, (d, v)) in cases.iter().zip(tickets.iter()).rev() {
        assert_eq!(dx12.readback(d)?, case.expected);
        assert_eq!(vulkan.readback(v)?, case.expected);
    }
    assert_eq!(dx12.pending_count(), 0);
    assert_eq!(vulkan.pending_count(), 0);
    assert!(dx12.readback(&tickets[0].0).is_err());
    assert!(vulkan.readback(&tickets[0].1).is_err());
    // Dropping an observation does not retire in-flight work. Context teardown
    // must still wait safely and release that unobserved final batch.
    drop(dx12.submit(&cases[0])?);
    drop(vulkan.submit(&cases[0])?);
    assert_eq!(dx12.pending_count(), 1);
    assert_eq!(vulkan.pending_count(), 1);
    drop(dx12);
    drop(vulkan);
    dx12::assert_valid(&dx12_messages)?;
    assert!(vulkan_messages.lock().unwrap().is_empty());
    Ok(())
}
