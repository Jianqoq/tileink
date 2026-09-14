use super::{Result, dx12, submissions, vulkan};
#[path = "tests/cases.rs"]
mod cases;
use crate::wgpu::test_gpu::gpu_identity;
#[path = "tests/wgpu.rs"]
mod reference;

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
    use sha2::Digest;
    let mut report = Vec::new();
    for repetition in 0..3 {
        for (index, case) in cases::cases().into_iter().enumerate() {
            let outputs = [
                vulkan.execute(&case)?,
                dx12.execute(&case)?,
                wgpu_dx12.execute(&case)?,
                wgpu_vulkan.execute(&case)?,
            ];
            let mut hashes = Vec::new();
            for (route, bytes) in ["native-vulkan", "native-dx12", "wgpu-dx12", "wgpu-vulkan"]
                .into_iter()
                .zip(&outputs)
            {
                let difference = bytes.iter().zip(&case.expected).position(|(a, b)| a != b);
                assert!(
                    bytes.len() == case.expected.len() && difference.is_none(),
                    "{route} case {index} repetition {repetition} {:?}: first different byte {difference:?}",
                    case.params
                );
                hashes.push(serde_json::json!({"route":route, "sha256":sha2::Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}));
            }
            report.push(serde_json::json!({"case":index,"repetition":repetition,"entry":case.entry,
                "parameters":bytemuck::bytes_of(&case.params),"bytes":case.expected.len(),"outputs":hashes,
                "different_pixels":0,"max_channel_delta":0}));
        }
    }
    drop(dx12);
    drop(vulkan);
    dx12::assert_valid(&dx12_messages)?;
    let messages = vulkan_messages.lock().unwrap();
    assert!(messages.is_empty(), "Vulkan validation: {messages:?}");
    eprintln!(
        "M3 four-API exact: {} cases x 3 repetitions x 4 APIs; zero differing bytes",
        report.len() / 3
    );
    if let Some(path) = std::env::var_os("TILEINK_NATIVE_GPU_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "physical_gpu_luid":identity,"cases":report.len()/3,"repetitions":3,"routes":4,
                "native_validation":"passed including teardown","frames":report
            }))?,
        )?;
    }
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

#[path = "tests/batches.rs"]
mod batches;

#[path = "tests/scatter.rs"]
mod scatter;

#[path = "tests/cumsum_gpu.rs"]
mod cumsum;

#[path = "tests/scan_gpu.rs"]
mod scan;

#[path = "tests/scan_count_gpu.rs"]
mod scan_count;

#[path = "tests/scan_emit_gpu.rs"]
mod scan_emit;

#[path = "tests/padded_tail.rs"]
mod padded_tail;

#[path = "tests/coarse_prefix_gpu.rs"]
mod coarse_prefix;

#[path = "tests/coarse_emit_gpu.rs"]
mod coarse_emit;

#[allow(dead_code)]
#[path = "../../../build/gpu_constants.rs"]
mod hlsl_constants;

#[path = "tests/coarse_count_gpu.rs"]
mod coarse_count;

#[path = "tests/four_api.rs"]
mod four_api;
#[path = "tests/pixel_math_gpu.rs"]
mod pixel_math;

#[path = "tests/geometry_math_gpu.rs"]
mod geometry_math;

#[path = "tests/blend_math_gpu.rs"]
mod blend_math;

#[path = "tests/gradient_gpu.rs"]
mod gradient;

#[path = "tests/texture_gpu.rs"]
mod texture;

#[path = "tests/sampler_gpu.rs"]
mod sampler;

#[path = "tests/pattern_gpu.rs"]
mod pattern;

#[path = "tests/filter_gpu.rs"]
mod filter;

#[path = "tests/filter_color_gpu.rs"]
mod filter_color;

#[path = "tests/filter_inputs_gpu.rs"]
mod filter_inputs;

#[path = "tests/filter_morphology_gpu.rs"]
mod filter_morphology;

#[path = "tests/filter_displacement_gpu.rs"]
mod filter_displacement;

#[path = "tests/filter_transfer_gpu.rs"]
mod filter_transfer;

#[path = "tests/filter_convolve_gpu.rs"]
mod filter_convolve;

#[path = "tests/filter_resample_gpu.rs"]
mod filter_resample;

#[path = "tests/filter_blur_gpu.rs"]
mod filter_blur;

#[path = "tests/filter_lighting_gpu.rs"]
mod filter_lighting;

#[path = "tests/filter_rectangle_gpu.rs"]
mod filter_rectangle;
