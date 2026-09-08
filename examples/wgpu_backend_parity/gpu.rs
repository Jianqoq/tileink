use peniko::Color;
use serde_json::json;
use tileink::WgpuRenderer;

use crate::Result;

pub struct Route {
    pub name: String,
    pub renderer: WgpuRenderer,
    pub identity: String,
    pub metadata: serde_json::Value,
}

pub fn create(
    instance: &wgpu::Instance,
    backend: wgpu::Backend,
    portable: bool,
    luid: Option<&str>,
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
        match create_on_adapter(&adapter, backend, portable) {
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

fn create_on_adapter(
    adapter: &wgpu::Adapter,
    backend: wgpu::Backend,
    portable: bool,
) -> Result<Route> {
    let info = adapter.get_info();
    if info.backend != backend {
        return Err("adapter API differs from explicit request".into());
    }
    let optional = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    if !portable
        && !adapter
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return Err("native texture path unavailable; refusing portable fallback".into());
    }
    let required_features = if portable {
        wgpu::Features::empty()
    } else {
        adapter.features() & optional
    };
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
        "limits": format!("{:?}", device.limits()),
        "rgba8unorm_features": format!("{:?}", adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm)),
    });
    Ok(Route {
        name,
        renderer: WgpuRenderer::new(&device, &queue, 1, 1, Color::TRANSPARENT),
        identity,
        metadata,
    })
}

#[cfg(windows)]
fn physical_identity(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Result<String> {
    let luid = match adapter.get_info().backend {
        wgpu::Backend::Dx12 => {
            // Read-only query through a live guard; no ownership is transferred or destroyed.
            let hal =
                unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }.ok_or("missing DX12 device")?;
            let luid = unsafe { hal.raw_device().GetAdapterLuid() };
            let mut bytes = [0; 8];
            bytes[..4].copy_from_slice(&luid.LowPart.to_le_bytes());
            bytes[4..].copy_from_slice(&luid.HighPart.to_le_bytes());
            bytes
        }
        wgpu::Backend::Vulkan => {
            // Keep the adapter/instance alive throughout the physical-device property query.
            let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }
                .ok_or("missing Vulkan adapter")?;
            let mut id = ash::vk::PhysicalDeviceIDProperties::default();
            let mut properties = ash::vk::PhysicalDeviceProperties2::default().push_next(&mut id);
            unsafe {
                hal.shared_instance()
                    .raw_instance()
                    .get_physical_device_properties2(hal.raw_physical_device(), &mut properties)
            };
            if id.device_luid_valid == 0 {
                return Err("Vulkan did not provide a valid device LUID; same-GPU parity cannot be certified".into());
            }
            id.device_luid
        }
        _ => return Err("unsupported identity query".into()),
    };
    Ok(luid.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(windows))]
fn physical_identity(_adapter: &wgpu::Adapter, _device: &wgpu::Device) -> Result<String> {
    Err("the DX12/Vulkan reference pair requires Windows".into())
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
