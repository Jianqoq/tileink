//! Explicit API and physical GPU selection for reproducible GPU benchmarks.

#[path = "gpu_identity.rs"]
mod gpu_identity;

pub fn device(
    api: &str,
    portable: bool,
    timestamps: bool,
    memory_hints: wgpu::MemoryHints,
) -> (serde_json::Value, wgpu::Device, wgpu::Queue) {
    let backends = match api {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        _ => panic!("benchmark API must be vulkan or dx12"),
    };
    let expected = std::env::var("TILEINK_BENCH_GPU")
        .expect("set TILEINK_BENCH_GPU to the physical LUID (Windows) or device UUID (Linux)")
        .to_ascii_lowercase();
    let mut descriptor = wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    // Resolve once: the loader and recorded digest must refer to exactly one file.
    let compiler_path = (api == "dx12").then(|| {
        std::fs::canonicalize(
            std::env::var("TILEINK_PARITY_DXCOMPILER")
                .expect("DX12 benchmarks require pinned TILEINK_PARITY_DXCOMPILER"),
        )
        .expect("pinned DXC path cannot be resolved")
    });
    if let Some(path) = &compiler_path {
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: path.to_str().expect("DXC path must be UTF-8").to_owned(),
        };
    }
    let instance = wgpu::Instance::new(descriptor);
    let mut features = if portable {
        wgpu::Features::empty()
    } else {
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
    };
    if timestamps {
        features |= wgpu::Features::TIMESTAMP_QUERY;
    }
    let cache_requested = pipeline_cache_requested();
    let mut observed = Vec::new();
    for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
        let info = adapter.get_info();
        if !matches!(
            info.device_type,
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
        ) || !adapter.features().contains(features)
        {
            continue;
        }
        let cache_enabled =
            cache_requested && adapter.features().contains(wgpu::Features::PIPELINE_CACHE);
        let features = features
            | if cache_enabled {
                wgpu::Features::PIPELINE_CACHE
            } else {
                wgpu::Features::empty()
            };
        let requested = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("benchmark measurements"),
            required_features: features,
            required_limits: adapter.limits(),
            memory_hints: memory_hints.clone(),
            ..Default::default()
        }));
        let (device, queue) = match requested {
            Ok(pair) => pair,
            Err(error) => {
                observed.push(format!("{}: {error}", info.name));
                continue;
            }
        };
        let identity = match gpu_identity::physical_identity(&adapter, &device) {
            Ok(identity) => identity,
            Err(error) => {
                observed.push(format!("{}: {error}", info.name));
                continue;
            }
        };
        if identity != expected {
            observed.push(identity);
            continue;
        }
        let compiler = if let Some(path) = &compiler_path {
            use sha2::{Digest, Sha256};
            let sha256: String =
                Sha256::digest(std::fs::read(path).expect("cannot read pinned DXC"))
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
            serde_json::json!({"mode":"DynamicDxc", "path":path, "sha256":sha256})
        } else {
            serde_json::json!({"mode":"WGPU WGSL to SPIR-V"})
        };
        let metadata = serde_json::json!({
            "physical_identity":identity, "api":format!("{:?}",info.backend),
            "name":info.name, "vendor":info.vendor, "device":info.device,
            "driver":info.driver, "driver_info":info.driver_info,
            "features":format!("{features:?}"), "memory_hints":format!("{memory_hints:?}"),
            "runtime_compiler":compiler,
            "pipeline_cache_requested":cache_requested, "pipeline_cache_feature_enabled":cache_enabled,
            "rgba8unorm_features":format!("{:?}",adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm)),
        });
        eprintln!("benchmark adapter: {metadata}; portable textures: {portable}");
        return (metadata, device, queue);
    }
    panic!("requested {api} GPU {expected} unavailable; observed identities: {observed:?}");
}

fn pipeline_cache_requested() -> bool {
    match std::env::var("TILEINK_BENCH_PIPELINE_CACHE").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => false,
        _ => panic!("TILEINK_BENCH_PIPELINE_CACHE must be 0 or 1"),
    }
}

/// Preserve ordinary diagnostic device selection without constructing a throwaway Renderer.
/// An explicit texture mode is strict; `None` retains the default capability-driven mode.
#[allow(dead_code)]
pub fn default_device(portable: Option<bool>) -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("request default benchmark adapter");
    let supported = adapter.features();
    let native = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    let mut features = match portable {
        Some(true) => wgpu::Features::empty(),
        Some(false) => {
            assert!(
                supported.contains(native),
                "requested native textures unavailable"
            );
            native
        }
        None => supported & (native | wgpu::Features::TIMESTAMP_QUERY),
    };
    let cache_requested = pipeline_cache_requested();
    if cache_requested {
        features |= supported & wgpu::Features::PIPELINE_CACHE;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink default benchmark device"),
        required_features: features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))
    .expect("request default benchmark device");
    eprintln!(
        "default benchmark adapter: {:?}; pipeline_cache_requested={cache_requested}; pipeline_cache_feature_enabled={}",
        adapter.get_info(),
        features.contains(wgpu::Features::PIPELINE_CACHE),
    );
    (device, queue)
}
