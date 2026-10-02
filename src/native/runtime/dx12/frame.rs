//! Per-submission D3D12 command and resource ownership.
use super::{Dispatch, Result};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_SAMPLE_DESC};

#[derive(Clone)]
pub struct Frame {
    descriptors: Vec<ID3D12DescriptorHeap>,
    device: ID3D12Device,
    _allocator: ID3D12CommandAllocator,
    pub list: ID3D12GraphicsCommandList,
    buffers: Vec<ID3D12Resource>,
    readback: Option<ID3D12Resource>,
    size: usize,
}
impl Frame {
    pub fn record(
        device: &ID3D12Device,
        signature: &ID3D12RootSignature,
        pipeline: &ID3D12PipelineState,
        case: &Dispatch,
    ) -> Result<Self> {
        unsafe {
            let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            let mut frame = Self {
                device: device.clone(),
                _allocator: allocator,
                list,
                buffers: Vec::new(),
                descriptors: Vec::new(),
                readback: None,
                size: case.destination().len(),
            };
            let size = case.destination().len();
            let destination = frame.buffer(
                size,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                None,
            )?;
            let initial = frame.buffer(
                size,
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(case.destination()),
            )?;
            let source = frame.buffer(
                case.source().len(),
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(case.source()),
            )?;
            let params = if let Some(params) = case.params() {
                Some(frame.buffer(
                    256,
                    D3D12_HEAP_TYPE_UPLOAD,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    D3D12_RESOURCE_FLAG_NONE,
                    Some(bytemuck::bytes_of(params)),
                )?)
            } else {
                None
            };
            let readback = frame.buffer(
                size,
                D3D12_HEAP_TYPE_READBACK,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_FLAG_NONE,
                None,
            )?;
            // Default-heap buffers start COMMON; an explicit transition avoids
            // relying on the initial state ignored by current D3D12 runtimes.
            frame.transition(
                &destination,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            frame
                .list
                .CopyBufferRegion(&destination, 0, &initial, 0, size as u64);
            frame.transition(
                &destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            );
            frame.list.SetComputeRootSignature(signature);
            frame.list.SetPipelineState(pipeline);
            if let Some((width, bytes)) = case.texture() {
                frame.record_texture(width, bytes)?;
            }
            frame
                .list
                .SetComputeRootUnorderedAccessView(0, destination.GetGPUVirtualAddress());
            frame
                .list
                .SetComputeRootShaderResourceView(1, source.GetGPUVirtualAddress());
            if let Some(params) = params {
                frame
                    .list
                    .SetComputeRootConstantBufferView(2, params.GetGPUVirtualAddress());
            }
            // An empty upload preserves the destination without issuing a zero
            // Dispatch (which the D3D12 validation layer reports as a warning).
            if case.workgroups() != 0 {
                frame.list.Dispatch(case.workgroups(), 1, 1);
            }
            frame.transition(
                &destination,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            frame
                .list
                .CopyBufferRegion(&readback, 0, &destination, 0, size as u64);
            frame.list.Close()?;

            frame.readback = Some(readback);
            Ok(frame)
        }
    }
    pub fn readback(&self) -> Result<Vec<u8>> {
        super::buffer::read(self.readback.as_ref().unwrap(), self.size)
    }
    fn buffer(
        &mut self,
        size: usize,
        heap: D3D12_HEAP_TYPE,
        state: D3D12_RESOURCE_STATES,
        flags: D3D12_RESOURCE_FLAGS,
        contents: Option<&[u8]>,
    ) -> Result<ID3D12Resource> {
        let resource = super::buffer::create(&self.device, size, heap, state, flags, contents)?;
        self.buffers.push(resource.clone());
        Ok(resource)
    }
    fn transition(
        &self,
        resource: &ID3D12Resource,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) {
        super::buffer::transition(&self.list, resource, before, after);
    }
}

#[path = "texture.rs"]
mod texture;
