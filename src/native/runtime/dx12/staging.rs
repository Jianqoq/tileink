//! Upload resources are recycled only after the owning submission's fence completes.
use super::{Result, buffer};
use windows::Win32::Graphics::Direct3D12::*;

pub(super) fn prepare(
    device: &ID3D12Device,
    contents: &[u8],
    cached: &mut super::buffer_cache::Available,
) -> Result<super::buffer_cache::Buffer> {
    prepare_with(device, contents.len(), cached, |mapped| {
        mapped.write(0, contents)
    })
}

/// Writes directly into retired upload storage, avoiding a second CPU packing buffer.
pub(super) fn prepare_with(
    device: &ID3D12Device,
    size: usize,
    cached: &mut super::buffer_cache::Available,
    write: impl FnOnce(&mut Mapped<'_>),
) -> Result<super::buffer_cache::Buffer> {
    let resource = match cached.take(size) {
        Some(resource) => resource,
        None => buffer::create(
            device,
            size.checked_next_power_of_two()
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
        let mut mapped = Mapped {
            resource: &resource.resource,
            pointer: pointer.cast(),
            size,
        };
        write(&mut mapped);
    }
    Ok(resource)
}

pub(super) struct Mapped<'a> {
    resource: &'a ID3D12Resource,
    pointer: *mut u8,
    size: usize,
}

impl Mapped<'_> {
    pub fn write(&mut self, offset: usize, bytes: &[u8]) {
        assert!(offset <= self.size && bytes.len() <= self.size - offset);
        // The mapping is exclusively owned until Drop. Padding need not be initialized:
        // texture copies consume only the texels described by each footprint.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.pointer.add(offset), bytes.len());
        }
    }
}

impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        unsafe {
            self.resource.Unmap(
                0,
                Some(&D3D12_RANGE {
                    Begin: 0,
                    End: self.size,
                }),
            );
        }
    }
}
