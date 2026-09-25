//! Reusable DEFAULT-heap buffers; only completed frames return resources here.
//! D3D12 buffers decay to COMMON after ExecuteCommandLists completes, even after
//! explicit transitions. Fence-confirmed retirement therefore needs no extra
//! end-of-frame COMMON barrier. Textures and upload heaps are not pooled here.
use super::{Result, buffer};
use windows::Win32::Graphics::Direct3D12::*;

pub(super) fn prepare(
    device: &ID3D12Device,
    size: usize,
    cached: &mut super::buffer_cache::Available,
) -> Result<super::buffer_cache::Buffer> {
    match cached.take(size) {
        Some(resource) => Ok(resource),
        None => buffer::create(
            device,
            size.checked_next_power_of_two()
                .ok_or("DX12 storage capacity overflow")?,
            D3D12_HEAP_TYPE_DEFAULT,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            None,
        )
        .map(Into::into),
    }
}
