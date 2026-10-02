use crate::native::runtime::compute::TextureCopy;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::*;

/// Caller owns the resource leases and transitioned both textures to copy states.
pub(super) unsafe fn record(
    list: &ID3D12GraphicsCommandList,
    source: &ID3D12Resource,
    destination: &ID3D12Resource,
    copy: &TextureCopy,
) {
    unsafe {
        for layer in 0..copy.extent[2] {
            let mut src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(source.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: copy.source_origin[2] + layer,
                },
            };
            let mut dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(destination.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: copy.destination_origin[2] + layer,
                },
            };
            let region = D3D12_BOX {
                left: copy.source_origin[0],
                top: copy.source_origin[1],
                front: 0,
                right: copy.source_origin[0] + copy.extent[0],
                bottom: copy.source_origin[1] + copy.extent[1],
                back: 1,
            };
            list.CopyTextureRegion(
                &dst,
                copy.destination_origin[0],
                copy.destination_origin[1],
                0,
                &src,
                Some(&region),
            );
            ManuallyDrop::drop(&mut src.pResource);
            ManuallyDrop::drop(&mut dst.pResource);
        }
    }
}
