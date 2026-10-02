use super::*;
impl Dx12 {
    pub fn import_texture(
        &self,
        descriptor: crate::native::interop::dx12::TextureDescriptor,
    ) -> Result<(crate::native::runtime::texture::Allocation, [u32; 2])> {
        unsafe {
            let mut owner = None;
            descriptor.resource.GetDevice(&mut owner)?;
            let owner: ID3D12Device = owner.ok_or("DX12 image has no device")?;
            if owner.cast::<windows::core::IUnknown>()?.as_raw()
                != self.gpu.device.cast::<windows::core::IUnknown>()?.as_raw()
            {
                return Err("DX12 image belongs to another logical device".into());
            }
            let desc = descriptor.resource.GetDesc();
            // Import defaults are used by ordinary renders, so validate them just
            // like per-use overrides before any command can reference the image.
            super::synchronization::validate_state(&desc, descriptor.initial_state)?;
            super::synchronization::validate_state(&desc, descriptor.final_state)?;
            let size = texture_extent(&desc, self.limits().image_dimension)?;
            Ok((
                crate::native::runtime::texture::Allocation::Dx12(std::rc::Rc::new(
                    crate::native::runtime::texture::Dx12Allocation {
                        resource: descriptor.resource,
                        state: std::cell::Cell::new(descriptor.initial_state),
                        final_state: descriptor.final_state,
                    },
                )),
                size,
            ))
        }
    }
}
fn texture_extent(desc: &D3D12_RESOURCE_DESC, limit: u32) -> Result<[u32; 2]> {
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.DepthOrArraySize != 1 || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1 || desc.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM
        || !desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)
        // Root images also feed backdrop and filter SRVs; UAV alone is insufficient.
        || desc.Flags.contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)
    {
        return Err(
            "DX12 image requires single-mip RGBA8 UNORM with UAV and shader-read usage".into(),
        );
    }
    let size = [u32::try_from(desc.Width)?, desc.Height];
    crate::native::renderer::validate_size((size[0], size[1]), limit)?;
    Ok(size)
}
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    #[test]
    fn import_rejects_missing_shader_access_and_invalid_shape() {
        let mut desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 7,
            Height: 3,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        assert_eq!(texture_extent(&desc, 64).unwrap(), [7, 3]);
        desc.Flags |= D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE;
        assert!(texture_extent(&desc, 64).is_err());
        desc.Flags = D3D12_RESOURCE_FLAG_NONE;
        assert!(texture_extent(&desc, 64).is_err());
        desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        assert!(texture_extent(&desc, 64).is_err());
        desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        assert!(texture_extent(&desc, 2).is_err());
        desc.Width = 0;
        assert!(texture_extent(&desc, 64).is_err());
    }
}
