//! Upload resources are recycled only after the owning submission's fence completes.
use super::{Result, buffer};
use windows::Win32::Graphics::Direct3D12::*;

pub(super) fn prepare(
    device: &ID3D12Device,
    contents: &[u8],
    cached: &mut super::buffer_cache::Available,
) -> Result<super::buffer_cache::Buffer> {
    let resource = match cached.take(contents.len()) {
        Some(resource) => resource,
        None => buffer::create(
            device,
            contents
                .len()
                .checked_next_power_of_two()
                .ok_or("DX12 upload capacity overflow")?,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            D3D12_RESOURCE_FLAG_NONE,
            None,
        )?
        .into(),
    };
    unsafe {
        let mut pointer = std::ptr::null_mut();
        resource.resource.Map(
            0,
            Some(&D3D12_RANGE { Begin: 0, End: 0 }),
            Some(&mut pointer),
        )?;
        std::ptr::copy_nonoverlapping(contents.as_ptr(), pointer.cast(), contents.len());
        resource.resource.Unmap(
            0,
            Some(&D3D12_RANGE {
                Begin: 0,
                End: contents.len(),
            }),
        );
    }
    Ok(resource)
}
