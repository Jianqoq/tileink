#[path = "../common/gpu_identity.rs"]
pub(super) mod gpu_identity;
use gpu_identity::physical_identity;

use peniko::Color;
use serde_json::json;
use tileink::WgpuRenderer;

use crate::Result;

pub struct Route {
    pub name: String,
    pub renderer: WgpuRenderer,
    pub identity: String,
    pub metadata: serde_json::Value,
    require_precompiled_fine: bool,
}

#[cfg(all(test, windows))]
pub fn create(
    instance: &wgpu::Instance,
    backend: wgpu::Backend,
    portable: bool,
    luid: Option<&str>,
) -> Result<Route> {
    create_with_fine(
        instance,
        backend,
        portable,
        luid,
        super::options::Dx12Fine::Runtime,
    )
}

pub fn create_with_fine(
    instance: &wgpu::Instance,
    backend: wgpu::Backend,
    portable: bool,
    luid: Option<&str>,
    fine: super::options::Dx12Fine,
) -> Result<Route> {
    let backends = match backend {
        wgpu::Backend::Dx12 => wgpu::Backends::DX12,
        wgpu::Backend::Vulkan => wgpu::Backends::VULKAN,
        _ => return Err("parity reference requires explicit DX12 or Vulkan".into()),
    };
    let mut adapters = pollster::block_on(instance.enumerate_adapters(backends));
    adapters.retain(|adapter| {
        matches!(
            adapter.get_info().device_type,
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
        )
    });
    adapters.sort_by_key(|adapter| {
        let info = adapter.get_info();
        (
            info.device_type != wgpu::DeviceType::DiscreteGpu,
            info.vendor,
            info.device,
            info.name,
        )
    });
    let mut failures = Vec::new();
    for adapter in adapters {
        match create_on_adapter(&adapter, backend, portable, fine) {
            Ok(route) if luid.is_none_or(|expected| route.identity == expected) => {
                return Ok(route);
            }
            Ok(_) => {}
            Err(error) => failures.push(error.to_string()),
        }
    }
    Err(format!(
        "hardware {backend:?} with LUID {luid:?} unavailable: {}",
        failures.join("; ")
    )
    .into())
}

fn requested_features(
    available: wgpu::Features,
    backend: wgpu::Backend,
    portable: bool,
    fine: super::options::Dx12Fine,
) -> Result<(wgpu::Features, bool)> {
    let optional = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    if !portable && !available.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
        return Err("native texture path unavailable; refusing portable fallback".into());
    }
    let mut required_features = if portable {
        wgpu::Features::empty()
    } else {
        available & optional
    };
    let require_precompiled_fine =
        fine == super::options::Dx12Fine::Precompiled && backend == wgpu::Backend::Dx12 && portable;
    if require_precompiled_fine {
        // Embedded fine DXIL has a 64-entry image table. Passthrough alone leaves
        // that table disabled and silently selects runtime WGSL instead.
        let dxil_features = wgpu::Features::PASSTHROUGH_SHADERS
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        if !available.contains(dxil_features) {
            return Err(
                "selected adapter lacks the precompiled DXIL texture-table contract".into(),
            );
        }
        required_features |= dxil_features;
    }
    Ok((required_features, require_precompiled_fine))
}

fn create_on_adapter(
    adapter: &wgpu::Adapter,
    backend: wgpu::Backend,
    portable: bool,
    fine: super::options::Dx12Fine,
) -> Result<Route> {
    let info = adapter.get_info();
    if info.backend != backend {
        return Err("adapter API differs from explicit request".into());
    }
    let (required_features, require_precompiled_fine) =
        requested_features(adapter.features(), backend, portable, fine)?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink explicit backend parity device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))?;
    let identity = physical_identity(adapter, &device)?;
    let name = format!(
        "wgpu-{}-{}",
        if backend == wgpu::Backend::Dx12 {
            "dx12"
        } else {
            "vulkan"
        },
        if portable {
            "portable-texture"
        } else {
            "native-texture"
        }
    );
    let metadata = json!({
        "route": name, "api": format!("{:?}", info.backend), "adapter": info.name,
        "vendor": info.vendor, "device": info.device, "driver": info.driver,
        "driver_info": info.driver_info, "physical_identity": identity,
        "requested_features": format!("{required_features:?}"),
        "precompiled_fine_required": require_precompiled_fine,
        "limits": format!("{:?}", device.limits()),
        "rgba8unorm_features": format!("{:?}", adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm)),
    });
    Ok(Route {
        name,
        renderer: WgpuRenderer::new(&device, &queue, 1, 1, Color::TRANSPARENT),
        identity,
        metadata,
        require_precompiled_fine,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
    fn rotated_pattern_coordinate_cancellation_matches() -> Result<()> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                        dxc_path: std::env::var("TILEINK_PARITY_DXCOMPILER")?,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/painting/context/with-pattern-and-transform-in-use.svg");
        let (scene, _, _) = super::super::common::load_svg_scene(path, 300)?;
        let mut reference = create(&instance, wgpu::Backend::Dx12, false, None)?;
        let mut other = create(
            &instance,
            wgpu::Backend::Vulkan,
            false,
            Some(&reference.identity),
        )?;
        reference.renderer.render(&scene);
        other.renderer.render(&scene);
        let difference =
            super::super::pixels::compare(&reference.renderer.image(), &other.renderer.image())?;
        assert_eq!(
            difference.pixels, 0,
            "rotated pattern must sample the same texels: {difference:?}"
        );
        Ok(())
    }

    #[test]
    fn wgpu_explicit_dx12_empty_scene_readback() -> Result<()> {
        if std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() != Ok("1") {
            return Ok(());
        }
        let _ = env_logger::try_init();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: std::env::var("TILEINK_PARITY_DXCOMPILER")
                        .map(|dxc_path| wgpu::Dx12Compiler::DynamicDxc { dxc_path })
                        .unwrap_or_default(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let mut route = create(&instance, wgpu::Backend::Dx12, false, None)?;
        // Clearing a resized target used to invalidate the DX12 device: filter
        // inputs were bound as both storage and sampled textures (UAV | SRV).
        let scene = tileink::Canvas::new(17, 15, 1.0);
        route.renderer.render(&scene);
        let image = route.renderer.image();
        assert_eq!((image.width, image.height), (17, 15));
        assert_eq!(image.pixels, vec![0; 17 * 15]);
        Ok(())
    }
}

impl Route {
    pub fn verify_fine_compiler(&self, initialized_precompiled: bool) -> Result<()> {
        // Retained variants own separate renderers; inspect the one actually executed.
        if self.require_precompiled_fine && !initialized_precompiled {
            return Err(format!(
                "{} requires embedded fine DXIL, but no precompiled pipeline was initialized",
                self.name
            )
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod compiler_tests {
    use super::super::options::Dx12Fine;
    use super::*;

    #[test]
    fn precompiled_fine_requires_the_complete_texture_table_contract() {
        let arrays = wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        let required = arrays | wgpu::Features::PASSTHROUGH_SHADERS;
        let (features, certify) =
            requested_features(required, wgpu::Backend::Dx12, true, Dx12Fine::Precompiled).unwrap();
        assert_eq!(features, required);
        assert!(certify);
        for missing in [
            wgpu::Features::PASSTHROUGH_SHADERS,
            wgpu::Features::TEXTURE_BINDING_ARRAY,
            wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
        ] {
            assert!(
                requested_features(
                    required - missing,
                    wgpu::Backend::Dx12,
                    true,
                    Dx12Fine::Precompiled
                )
                .is_err()
            );
        }
        for (backend, mode) in [
            (wgpu::Backend::Dx12, Dx12Fine::Runtime),
            (wgpu::Backend::Vulkan, Dx12Fine::Precompiled),
        ] {
            assert_eq!(
                requested_features(required, backend, true, mode).unwrap(),
                (wgpu::Features::empty(), false)
            );
        }
    }
}
