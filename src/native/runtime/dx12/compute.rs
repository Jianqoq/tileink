use super::{Result, buffer, compute_pipeline::Pipeline};
use crate::native::runtime::compute::ComputeBatch;
use std::collections::BTreeMap;
use windows::Win32::Graphics::Direct3D12::*;
#[derive(Clone)]
pub struct Frame {
    pub list: ID3D12GraphicsCommandList,
    _allocator: ID3D12CommandAllocator,
    _buffers: Vec<ID3D12Resource>,
    _heap: Option<ID3D12DescriptorHeap>,
    _pipelines: Vec<Pipeline>,
    readbacks: Vec<(ID3D12Resource, usize)>,
}
impl Frame {
    pub fn record(
        device: &ID3D12Device,
        batch: &ComputeBatch,
        pipelines: &BTreeMap<&'static str, Pipeline>,
    ) -> Result<Self> {
        unsafe {
            let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            let mut frame = Self {
                list,
                _allocator: allocator,
                _buffers: Vec::new(),
                _heap: None,
                _pipelines: Vec::new(),
                readbacks: Vec::new(),
            };
            let mut gpu = Vec::new();
            for input in batch.buffers() {
                let size = input
                    .bytes
                    .len()
                    .div_ceil(D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize)
                    * D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize;
                let resource = buffer::create(
                    device,
                    size,
                    D3D12_HEAP_TYPE_DEFAULT,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                    None,
                )?;
                let upload = buffer::create(
                    device,
                    input.bytes.len(),
                    D3D12_HEAP_TYPE_UPLOAD,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    D3D12_RESOURCE_FLAG_NONE,
                    Some(&input.bytes),
                )?;
                buffer::transition(
                    &frame.list,
                    &resource,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                );
                frame
                    .list
                    .CopyBufferRegion(&resource, 0, &upload, 0, input.bytes.len() as u64);
                frame._buffers.extend([resource.clone(), upload]);
                gpu.push(resource);
            }
            let count = batch
                .passes()
                .iter()
                .try_fold(0usize, |n, p| n.checked_add(p.bindings.len()))
                .ok_or("native descriptor count overflow")?;
            if count > 1_000_000 {
                return Err("native DX12 shader-visible heap capacity exceeded".into());
            }
            let mut states = vec![D3D12_RESOURCE_STATE_COPY_DEST; gpu.len()];
            if count != 0 {
                let heap: ID3D12DescriptorHeap =
                    device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                        NumDescriptors: count as u32,
                        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                        NodeMask: 0,
                    })?;
                frame.list.SetDescriptorHeaps(&[Some(heap.clone())]);
                let step = device
                    .GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
                    as usize;
                let base = heap.GetCPUDescriptorHandleForHeapStart();
                let visible = heap.GetGPUDescriptorHandleForHeapStart();
                let mut index = 0;
                for pass in batch.passes() {
                    let pipeline = &pipelines[pass.shader.entry];
                    frame._pipelines.push(pipeline.clone());
                    frame.list.SetComputeRootSignature(&pipeline.signature);
                    frame.list.SetPipelineState(&pipeline.state);
                    for (id, state) in super::compute_bindings::required_states(pass) {
                        let resource = &gpu[id];
                        if states[id] != state {
                            buffer::transition(&frame.list, resource, states[id], state);
                            states[id] = state;
                        } else if state == D3D12_RESOURCE_STATE_UNORDERED_ACCESS {
                            buffer::uav_barrier(&frame.list, resource);
                        }
                    }
                    let start = index;
                    for (binding, id) in &pass.bindings {
                        let id = id.index();
                        let resource = &gpu[id];
                        let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                            ptr: base.ptr + index * step,
                        };
                        super::compute_bindings::write(
                            device,
                            binding,
                            resource,
                            (batch.buffers()[id].bytes.len() / 4) as u32,
                            handle,
                        );
                        index += 1;
                    }
                    frame.list.SetComputeRootDescriptorTable(
                        0,
                        D3D12_GPU_DESCRIPTOR_HANDLE {
                            ptr: visible.ptr + (start * step) as u64,
                        },
                    );
                    if pipeline.grid {
                        let grid = [pass.grid[0], pass.grid[1], pass.grid[2], 0];
                        frame
                            .list
                            .SetComputeRoot32BitConstants(1, 4, grid.as_ptr().cast(), 0);
                    }
                    frame
                        .list
                        .Dispatch(pass.grid[0], pass.grid[1], pass.grid[2]);
                }
                frame._heap = Some(heap);
            }
            for id in batch.outputs() {
                let id = id.index();
                let size = batch.buffers()[id].bytes.len();
                let readback = buffer::create(
                    device,
                    size,
                    D3D12_HEAP_TYPE_READBACK,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_FLAG_NONE,
                    None,
                )?;
                if states[id] != D3D12_RESOURCE_STATE_COPY_SOURCE {
                    buffer::transition(
                        &frame.list,
                        &gpu[id],
                        states[id],
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                    );
                    states[id] = D3D12_RESOURCE_STATE_COPY_SOURCE;
                }
                frame
                    .list
                    .CopyBufferRegion(&readback, 0, &gpu[id], 0, size as u64);
                frame.readbacks.push((readback, size));
            }
            frame.list.Close()?;
            Ok(frame)
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        self.readbacks
            .iter()
            .map(|(resource, size)| buffer::read(resource, *size))
            .collect()
    }
}
