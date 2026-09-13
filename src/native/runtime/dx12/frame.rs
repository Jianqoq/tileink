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
                size: case.destination.len(),
            };
            let size = case.destination.len();
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
                Some(&case.destination),
            )?;
            let source = frame.buffer(
                case.source.len(),
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(&case.source),
            )?;
            let params = frame.buffer(
                256,
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(bytemuck::bytes_of(&case.params)),
            )?;
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
            if case.entry == "sample_words" {
                let start = case.params.source_offset as usize;
                let end = start + case.params.value[2] as usize * 4;
                frame.record_texture(case.params.value[2], &case.source[start..end])?;
            }
            frame
                .list
                .SetComputeRootUnorderedAccessView(0, destination.GetGPUVirtualAddress());
            frame
                .list
                .SetComputeRootShaderResourceView(1, source.GetGPUVirtualAddress());
            frame
                .list
                .SetComputeRootConstantBufferView(2, params.GetGPUVirtualAddress());
            frame
                .list
                .Dispatch(case.params.count.div_ceil(64).max(1), 1, 1);
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
        let size = self.size;
        let readback = self.readback.as_ref().unwrap();
        unsafe {
            let mut pointer = std::ptr::null_mut();
            readback.Map(
                0,
                Some(&D3D12_RANGE {
                    Begin: 0,
                    End: size,
                }),
                Some(&mut pointer),
            )?;
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
            readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            Ok(bytes)
        }
    }
    fn buffer(
        &mut self,
        size: usize,
        heap: D3D12_HEAP_TYPE,
        state: D3D12_RESOURCE_STATES,
        flags: D3D12_RESOURCE_FLAGS,
        contents: Option<&[u8]>,
    ) -> Result<ID3D12Resource> {
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
            self.device.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                state,
                None,
                &mut resource,
            )?;
            let resource: ID3D12Resource = resource.unwrap();
            if let Some(contents) = contents {
                let mut pointer = std::ptr::null_mut();
                resource.Map(
                    0,
                    Some(&D3D12_RANGE { Begin: 0, End: 0 }),
                    Some(&mut pointer),
                )?;
                std::ptr::copy_nonoverlapping(contents.as_ptr(), pointer.cast(), contents.len());
                resource.Unmap(0, None);
            }
            self.buffers.push(resource.clone());
            Ok(resource)
        }
    }
    fn transition(
        &self,
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
            self.list.ResourceBarrier(std::slice::from_ref(&barrier));
            ManuallyDrop::drop(&mut (*barrier.Anonymous.Transition).pResource);
        }
    }
}

#[path = "texture.rs"]
mod texture;
