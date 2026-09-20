//! Cross-process WGSL evidence for native-only Metal builds; no runtime fallback.
#![cfg(all(target_os = "macos", feature = "wgpu"))]
#[allow(dead_code)]
#[path = "../src/shared/filter_config.rs"]
mod filter_config;
#[allow(dead_code)]
#[path = "../examples/common/benchmark_gpu.rs"]
mod gpu;
#[allow(dead_code)]
#[path = "../src/shared/gpu_constants.rs"]
mod gpu_constants;
#[allow(dead_code)]
#[path = "../src/wgpu/shader_variants.rs"]
mod shader_variants;
use gpu_constants::SDF_RECORD_WORDS;
const SDF_PROBE_REQUEST_WORDS: u32 = 12;
#[path = "shaders/sdf_cases.rs"]
mod sdf_cases;
use sha2::{Digest, Sha256};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "explicit same-device Metal math certification; writes target/metal-validation/math"]
fn export_production_wgsl_sdf_reference() -> Result<(), Box<dyn std::error::Error>> {
    let (records, requests, oracle) = sdf_cases::cases();
    let count = requests.len() / SDF_PROBE_REQUEST_WORDS as usize;
    let mut config = vec![0u8; std::mem::size_of::<filter_config::FilterConfig>()];
    let offset = std::mem::offset_of!(filter_config::FilterConfig, pixel_count);
    config[offset..offset + 4].copy_from_slice(&(count as u32).to_le_bytes());
    let (identity, device, queue) =
        gpu::device("metal", false, false, wgpu::MemoryHints::MemoryUsage);
    let source = format!(
        "{}\nconst SDF_PROBE_REQUEST_WORDS:u32=12u;\nconst SDF_PROBE_AFFINE_WORD:u32=4u;\n{}",
        shader_variants::patch_image_resource_shader_source(
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter.wgsl")),
            false
        ),
        include_str!("shaders/sdf.wgsl")
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production SDF reference"),
        source: wgpu::ShaderSource::Wgsl(source.clone().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("sdf_coverage_words"),
        compilation_options: Default::default(),
        cache: None,
    });
    let buffer = |bytes: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    };
    let uniform = buffer(&config, wgpu::BufferUsages::UNIFORM);
    let paint = buffer(bytemuck::cast_slice(&records), wgpu::BufferUsages::STORAGE);
    let positions = buffer(bytemuck::cast_slice(&requests), wgpu::BufferUsages::STORAGE);
    let output = buffer(
        bytemuck::cast_slice(&vec![0xa1b2c3d4u32; count + 4]),
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let bindings =
        [(0, &uniform), (5, &positions), (6, &output), (10, &paint)].map(|(binding, buffer)| {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &bindings,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((count as u32).div_ceil(256) + 1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    receive.recv()??;
    let pixels = readback.slice(..).get_mapped_range()?.to_vec();
    readback.unmap();
    assert_eq!(
        &pixels[..oracle.len() * 4],
        bytemuck::cast_slice::<u32, u8>(&oracle)
    );
    assert_eq!(
        &pixels[count * 4..],
        bytemuck::cast_slice::<u32, u8>(&[0xa1b2c3d4u32; 4])
    );
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/metal-validation/math");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(directory.join("sdf.bin"), &pixels)?;
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    std::fs::write(
        directory.join("sdf.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"identity":identity,"count":count,"records_sha256":digest(bytemuck::cast_slice(&records)),"requests_sha256":digest(bytemuck::cast_slice(&requests)),"wgsl_sha256":digest(source.as_bytes()),"output_sha256":digest(&pixels)}),
        )?,
    )?;
    Ok(())
}
