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
            let gpu = super::compute_resources::Resources::record(device, &frame.list, batch)?;
            let mut states = vec![D3D12_RESOURCE_STATE_COPY_DEST; batch.resources().len()];
            for (index, resource) in batch.resources().iter().enumerate() {
                if matches!(resource, Resource::Texture(texture) if texture.persistent.is_some()) {
                    states[index] = D3D12_RESOURCE_STATE_COMMON;
                }
            }
            let mut tables =
                super::compute_tables::Tables::new(device, &frame.list, batch.passes())?;
            for command in batch.commands() {
                let pass = match command {
                    crate::native::runtime::compute::Command::Dispatch(index) => {
                        &batch.passes()[*index]
                    }
                    crate::native::runtime::compute::Command::CopyTexture(copy) => {
                        for (id, state) in [
                            (copy.source.index(), D3D12_RESOURCE_STATE_COPY_SOURCE),
                            (copy.destination.index(), D3D12_RESOURCE_STATE_COPY_DEST),
                        ] {
                            if states[id] != state {
                                buffer::transition(&frame.list, gpu.get(id), states[id], state);
                                states[id] = state;
                            }
                        }
                        super::compute_copy::record(
                            &frame.list,
                            gpu.get(copy.source.index()),
                            gpu.get(copy.destination.index()),
                            copy,
                        );
                        continue;
                    }
                };
                let pipeline = &pipelines[pass.shader.entry];
                if batch.skip_initialization(pass) {
                    continue;
                }
                frame._pipelines.push(pipeline.clone());
                frame.list.SetComputeRootSignature(&pipeline.signature);
                frame.list.SetPipelineState(&pipeline.state);
                for (id, state) in super::compute_bindings::required_states(pass, batch.resources())
                {
                    // Upload heaps stay in GENERIC_READ; packed constants are immutable.
                    if gpu.uniform_offset(id).is_some() {
                        continue;
                    }
                    let resource = gpu.get(id);
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
                let size = batch.resources()[id].byte_len();
                if matches!(batch.resources()[id], Resource::Texture(_)) {
                    if states[id] != D3D12_RESOURCE_STATE_COPY_SOURCE {
                        buffer::transition(
                            &frame.list,
                            gpu.get(id),
                            states[id],
                            D3D12_RESOURCE_STATE_COPY_SOURCE,
                        );
                        states[id] = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    }
                    frame.readbacks.push(compute_texture::readback(
                        device,
                        &frame.list,
                        gpu.get(id),
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
                        gpu.get(id),
                        states[id],
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                    );
                    states[id] = D3D12_RESOURCE_STATE_COPY_SOURCE;
                }
                frame
                    .list
                    .CopyBufferRegion(&readback, 0, gpu.get(id), 0, size as u64);
                frame.readbacks.push(Readback::buffer(readback, size));
            }
            // Normalize persistent queue state even after copies or readback.
            for (id, resource) in batch.resources().iter().enumerate() {
                if matches!(resource, Resource::Texture(texture) if texture.persistent.is_some())
                    && states[id] != D3D12_RESOURCE_STATE_COMMON
                {
                    buffer::transition(
                        &frame.list,
                        gpu.get(id),
                        states[id],
                        D3D12_RESOURCE_STATE_COMMON,
                    );
                }
            }
            frame._buffers = gpu.into_owners();
            frame.list.Close()?;
            Ok(frame)
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        self.readbacks.iter().map(Readback::read).collect()
    }
}
