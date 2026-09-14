use super::super::compute::Resource;
use super::compute_texture::{self, Readback};
use super::{Result, buffer, compute_pipeline::Pipeline};
use crate::native::runtime::compute::ComputeBatch;
use std::collections::BTreeMap;
use windows::Win32::Graphics::Direct3D12::*;
#[derive(Clone)]
pub struct Frame {
    pub list: ID3D12GraphicsCommandList,
    _allocator: ID3D12CommandAllocator,
    _buffers: Vec<ID3D12Resource>,
    _heaps: Vec<ID3D12DescriptorHeap>,
    _pipelines: Vec<Pipeline>,
    readbacks: Vec<Readback>,
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
                _heaps: Vec::new(),
                _pipelines: Vec::new(),
                readbacks: Vec::new(),
            };
            let mut gpu = Vec::new();
            for input in batch.resources() {
                if matches!(input, Resource::Sampler(_) | Resource::TextureTable(_)) {
                    gpu.push(None);
                    continue;
                }
                if let Resource::Texture(input) = input {
                    let (texture, upload) = compute_texture::upload(device, &frame.list, input)?;
                    frame._buffers.extend([texture.clone(), upload]);
                    gpu.push(Some(texture));
                    continue;
                }
                let size = input
                    .bytes()
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
                    input.bytes().len(),
                    D3D12_HEAP_TYPE_UPLOAD,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    D3D12_RESOURCE_FLAG_NONE,
                    Some(input.bytes()),
                )?;
                buffer::transition(
                    &frame.list,
                    &resource,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                );
                frame
                    .list
                    .CopyBufferRegion(&resource, 0, &upload, 0, input.bytes().len() as u64);
                frame._buffers.extend([resource.clone(), upload]);
                gpu.push(Some(resource));
            }
            let mut states = vec![D3D12_RESOURCE_STATE_COPY_DEST; gpu.len()];
            let mut tables =
                super::compute_tables::Tables::new(device, &frame.list, batch.passes())?;
            for pass in batch.passes() {
                let pipeline = &pipelines[pass.shader.entry];
                frame._pipelines.push(pipeline.clone());
                frame.list.SetComputeRootSignature(&pipeline.signature);
                frame.list.SetPipelineState(&pipeline.state);
                for (id, state) in super::compute_bindings::required_states(pass, batch.resources())
                {
                    let resource = gpu[id].as_ref().unwrap();
                    if states[id] != state {
                        buffer::transition(&frame.list, resource, states[id], state);
                        states[id] = state;
                    } else if state == D3D12_RESOURCE_STATE_UNORDERED_ACCESS {
                        buffer::uav_barrier(&frame.list, resource);
                    }
                }
                tables.write(device, &frame.list, pass, batch.resources(), &gpu, pipeline);
                if let Some(root) = pipeline.grid {
                    let grid = [pass.grid[0], pass.grid[1], pass.grid[2], 0];
                    frame
                        .list
                        .SetComputeRoot32BitConstants(root, 4, grid.as_ptr().cast(), 0);
                }
                frame
                    .list
                    .Dispatch(pass.grid[0], pass.grid[1], pass.grid[2]);
            }
            frame._heaps = tables.into_heaps();
            for id in batch.outputs() {
                let id = id.index();
                let size = batch.resources()[id].bytes().len();
                if matches!(batch.resources()[id], Resource::Texture(_)) {
                    if states[id] != D3D12_RESOURCE_STATE_COPY_SOURCE {
                        buffer::transition(
                            &frame.list,
                            gpu[id].as_ref().unwrap(),
                            states[id],
                            D3D12_RESOURCE_STATE_COPY_SOURCE,
                        );
                        states[id] = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    }
                    frame.readbacks.push(compute_texture::readback(
                        device,
                        &frame.list,
                        gpu[id].as_ref().unwrap(),
                    )?);
                    continue;
                }
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
                        gpu[id].as_ref().unwrap(),
                        states[id],
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                    );
                    states[id] = D3D12_RESOURCE_STATE_COPY_SOURCE;
                }
                frame.list.CopyBufferRegion(
                    &readback,
                    0,
                    gpu[id].as_ref().unwrap(),
                    0,
                    size as u64,
                );
                frame.readbacks.push(Readback::buffer(readback, size));
            }
            frame.list.Close()?;
            Ok(frame)
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        self.readbacks.iter().map(Readback::read).collect()
    }
}
