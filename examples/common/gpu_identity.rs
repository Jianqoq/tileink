//! Physical GPU identity shared by reference certification and performance tools.

#[cfg(any(windows, target_os = "linux"))]
pub fn physical_identity(
    adapter: &wgpu::Adapter,
    _device: &wgpu::Device,
) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = match adapter.get_info().backend {
        #[cfg(windows)]
        wgpu::Backend::Dx12 => {
            // Query through a live guard; no resource ownership is transferred.
            let hal =
                unsafe { _device.as_hal::<wgpu::hal::api::Dx12>() }.ok_or("missing DX12 device")?;
            let luid = unsafe { hal.raw_device().GetAdapterLuid() };
            let mut bytes = vec![0; 8];
            bytes[..4].copy_from_slice(&luid.LowPart.to_le_bytes());
            bytes[4..].copy_from_slice(&luid.HighPart.to_le_bytes());
            bytes
        }
        wgpu::Backend::Vulkan => {
            // The adapter/instance guard remains alive throughout this read-only query.
            let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }
                .ok_or("missing Vulkan adapter")?;
            let mut id = ash::vk::PhysicalDeviceIDProperties::default();
            let mut properties = ash::vk::PhysicalDeviceProperties2::default().push_next(&mut id);
            unsafe {
                hal.shared_instance()
                    .raw_instance()
                    .get_physical_device_properties2(hal.raw_physical_device(), &mut properties)
            };
            #[cfg(windows)]
            {
                if id.device_luid_valid == 0 {
                    return Err("Vulkan did not provide a valid LUID; same-GPU identity cannot be certified".into());
                }
                id.device_luid.to_vec()
            }
            #[cfg(target_os = "linux")]
            {
                if id.device_uuid == [0; 16] {
                    return Err("Vulkan did not provide a valid device UUID".into());
                }
                id.device_uuid.to_vec()
            }
        }
        _ => return Err("unsupported physical GPU identity query".into()),
    };
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn physical_identity(
    _adapter: &wgpu::Adapter,
    _device: &wgpu::Device,
) -> Result<String, Box<dyn std::error::Error>> {
    Err("DX12/Vulkan physical identity is supported on Windows/Linux".into())
}

#[cfg(target_os = "macos")]
pub fn physical_identity(
    _adapter: &wgpu::Adapter,
    device: &wgpu::Device,
) -> Result<String, Box<dyn std::error::Error>> {
    use objc2_metal::MTLDevice;
    // SAFETY: identity query only; the Metal handle cannot escape the live guard.
    let guard =
        unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or("missing Metal device")?;
    Ok(format!("{:016x}", guard.raw_device().registryID()))
}
