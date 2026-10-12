use super::Result;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};
pub fn create(
    device: &ID3D12Device,
    size: usize,
    heap: D3D12_HEAP_TYPE,
    state: D3D12_RESOURCE_STATES,
    flags: D3D12_RESOURCE_FLAGS,
    contents: Option<&[u8]>,
) -> Result<ID3D12Resource> {
    // SAFETY: The buffer descriptor uses the checked allocation size and a live device; the output COM resource is retained by its owner.
    unsafe {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: flags,
            ..Default::default()
        };
        let properties = D3D12_HEAP_PROPERTIES {
            Type: heap,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        let mut resource = None;
        device.CreateCommittedResource(
            &raw const properties,
            D3D12_HEAP_FLAG_NONE,
            &raw const desc,
            state,
            None,
            &raw mut resource,
        )?;
        let resource: ID3D12Resource =
            resource.ok_or("DX12 buffer creation returned no resource")?;
        if let Some(contents) = contents {
            let mut pointer = std::ptr::null_mut();
            resource.Map(
                0,
                Some(&D3D12_RANGE { Begin: 0, End: 0 }),
                Some(&raw mut pointer),
            )?;
            std::ptr::copy_nonoverlapping(contents.as_ptr(), pointer.cast(), contents.len());
            resource.Unmap(0, None);
        }
        Ok(resource)
    }
}
pub fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    // SAFETY: The caller supplies a live same-device command list, resource and actual before/after states; the temporary COM barrier reference is released once.
    unsafe {
        let mut barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: ManuallyDrop::new(Some(resource.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
            ..Default::default()
        };
        list.ResourceBarrier(std::slice::from_ref(&barrier));
        ManuallyDrop::drop(&mut (*barrier.Anonymous.Transition).pResource);
    }
}
pub fn read(readback: &ID3D12Resource, size: usize) -> Result<Vec<u8>> {
    // SAFETY: Completion precedes readback; the retained buffer contains size readable bytes and is unmapped after copying.
    unsafe {
        let mut pointer = std::ptr::null_mut();
        readback.Map(
            0,
            Some(&D3D12_RANGE {
                Begin: 0,
                End: size,
            }),
            Some(&raw mut pointer),
        )?;
        let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
        readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        Ok(bytes)
    }
}
pub fn uav_barrier(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource) {
    // SAFETY: The resource and list belong to the same live device; the barrier reference is retained for recording and released once.
    unsafe {
        let mut barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                    pResource: ManuallyDrop::new(Some(resource.clone())),
                }),
            },
            ..Default::default()
        };
        list.ResourceBarrier(std::slice::from_ref(&barrier));
        ManuallyDrop::drop(&mut (*barrier.Anonymous.UAV).pResource);
    }
}
