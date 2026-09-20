//! Cross-build Windows certification: every executable contains exactly one renderer.
#![cfg(all(windows, any(feature = "wgpu", feature = "dx12", feature = "vulkan")))]

#[path = "../examples/common/mod.rs"]
mod common;
#[path = "windows_corpus/corpus.rs"]
mod corpus;
#[cfg_attr(feature = "wgpu", path = "windows_corpus/wgpu.rs")]
#[cfg_attr(not(feature = "wgpu"), path = "windows_corpus/native.rs")]
mod engine;
#[allow(dead_code)]
#[path = "../examples/wgpu_backend_parity/evidence.rs"]
mod evidence;
#[path = "../examples/wgpu/suite.rs"]
mod example_suite;
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
#[path = "../examples/wgpu_backend_parity/gpu.rs"]
mod gpu;
#[cfg(feature = "wgpu")]
use gpu::gpu_identity;
#[path = "../examples/common/layer_filter_scenes.rs"]
mod layer_filter_scenes;
#[allow(dead_code)]
#[path = "../examples/wgpu_backend_parity/options.rs"]
mod options;
#[allow(dead_code)]
#[path = "../examples/wgpu_backend_parity/pixels.rs"]
mod pixels;
#[cfg(feature = "wgpu")]
#[path = "../examples/wgpu_backend_parity/readback.rs"]
mod readback;
#[path = "windows_corpus/report.rs"]
mod report;
#[path = "../examples/wgpu_backend_parity/retained_contract.rs"]
mod retained_contract;
#[path = "../examples/wgpu_backend_parity/retained_sequence.rs"]
mod retained_sequence;
#[cfg_attr(
    feature = "wgpu",
    path = "../examples/wgpu_backend_parity/retained_wgpu.rs"
)]
#[cfg_attr(
    not(feature = "wgpu"),
    path = "../examples/wgpu_backend_parity/native/retained.rs"
)]
#[allow(dead_code)] // The shared fixture's metadata is represented by our engine manifest.
mod retained_variant;
#[path = "../examples/wgpu_backend_parity/svg.rs"]
mod svg;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires an explicitly pinned Windows GPU; use scripts/ps1/run_native_acceptance.ps1"]
fn run_selected_corpus() -> Result {
    let _ = env_logger::try_init();
    corpus::run()
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "hardware inventory for the Windows acceptance runner"]
fn list_adapters() -> Result {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut rows = Vec::new();
    for adapter in pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)) {
        let info = adapter.get_info();
        if !matches!(
            info.device_type,
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
        ) {
            continue;
        }
        let (device, _) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        rows.push(serde_json::json!({"name": info.name, "vendor": info.vendor,
            "device": info.device, "driver": info.driver, "driver_info": info.driver_info,
            "physical_identity": gpu_identity::physical_identity(&adapter, &device)?}));
    }
    if rows.is_empty() {
        return Err("no hardware adapters found".into());
    }
    evidence::write_new_json(
        &std::path::PathBuf::from(std::env::var("TILEINK_ACCEPTANCE_OUTPUT")?),
        &serde_json::json!(rows),
    )
}
