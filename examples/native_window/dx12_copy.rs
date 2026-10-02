use super::Result;
use std::mem::ManuallyDrop;
use windows::{Win32::Graphics::Direct3D12::*, core::Interface};
#[derive(Clone)]
pub struct Commands {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
}
impl Commands {
    pub fn new(device: &ID3D12Device) -> Result<Self> {
        unsafe {
            let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            list.Close()?;
            Ok(Self { allocator, list })
        }
    }
    pub fn submit(
        &self,
        queue: &ID3D12CommandQueue,
        source: &ID3D12Resource,
        destination: &ID3D12Resource,
    ) -> Result {
        unsafe {
            self.allocator.Reset()?;
            self.list.Reset(&self.allocator, None)?;
            transition(
                &self.list,
                destination,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            self.list.CopyResource(destination, source);
            transition(
                &self.list,
                destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PRESENT,
            );
            self.list.Close()?;
            queue.ExecuteCommandLists(&[Some(self.list.cast()?)]);
            Ok(())
        }
    }
}
fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
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
