//! Descriptor writes and per-pass resource access state planning.
#[cfg(test)]
use super::super::compute::ComputeBatch;
use crate::native::shaders::{Binding, BindingKind};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_FORMAT_R32_TYPELESS};

pub(super) unsafe fn write(
    device: &ID3D12Device,
    binding: &Binding,
    resource: &ID3D12Resource,
    uniform_offset: u64,
    word_count: u32,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe {
        match binding.kind {
            BindingKind::Sampler => unreachable!("sampler descriptors use their own heap"),
            BindingKind::Texture | BindingKind::TextureTable => device.CreateShaderResourceView(
                resource,
                Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_SRV {
                            MipLevels: 1,
                            ..Default::default()
                        },
                    },
                }),
                handle,
            ),
            BindingKind::TextureArray => device.CreateShaderResourceView(
                resource,
                Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                            MipLevels: 1,
                            ArraySize: u32::from(resource.GetDesc().DepthOrArraySize),
                            ..Default::default()
                        },
                    },
                }),
                handle,
            ),
            BindingKind::TextureWrite => device.CreateUnorderedAccessView(
                resource,
                None,
                Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                    Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
                    ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_UAV::default(),
                    },
                }),
                handle,
            ),
            BindingKind::Uniform => device.CreateConstantBufferView(
                Some(&D3D12_CONSTANT_BUFFER_VIEW_DESC {
                    BufferLocation: resource.GetGPUVirtualAddress() + uniform_offset,
                    SizeInBytes: binding
                        .size
                        .div_ceil(D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT)
                        * D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT,
                }),
                handle,
            ),
            BindingKind::Read => device.CreateShaderResourceView(
                resource,
                Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Buffer: D3D12_BUFFER_SRV {
                            FirstElement: 0,
                            NumElements: word_count,
                            StructureByteStride: 0,
                            Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                        },
                    },
                }),
                handle,
            ),
            BindingKind::Write => device.CreateUnorderedAccessView(
                resource,
                None,
                Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Buffer: D3D12_BUFFER_UAV {
                            FirstElement: 0,
                            NumElements: word_count,
                            StructureByteStride: 0,
                            CounterOffsetInBytes: 0,
                            Flags: D3D12_BUFFER_UAV_FLAG_RAW,
                        },
                    },
                }),
                handle,
            ),
        }
    }
}

pub(super) struct RequiredStates {
    by_resource: Vec<Option<D3D12_RESOURCE_STATES>>,
    touched: Vec<usize>,
}

impl RequiredStates {
    pub(super) fn new(resource_count: usize) -> Self {
        Self {
            by_resource: vec![None; resource_count],
            touched: Vec::new(),
        }
    }

    pub(super) fn collect(
        &mut self,
        pass: &super::super::compute::Pass,
        resources: &[super::super::compute::Resource],
    ) {
        debug_assert_eq!(self.by_resource.len(), resources.len());
        for id in self.touched.drain(..) {
            self.by_resource[id] = None;
        }
        for (binding, id) in &pass.bindings {
            let state = match binding.kind {
                BindingKind::Sampler => continue,
                BindingKind::Uniform => D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER,
                BindingKind::Read
                | BindingKind::Texture
                | BindingKind::TextureArray
                | BindingKind::TextureTable => D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                BindingKind::Write | BindingKind::TextureWrite => {
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS
                }
            };
            // A resource may be both CBV and SRV in one pass. Preserve every read state;
            // replacing it per descriptor loses one access class (root-cause fix).
            let images = match &resources[id.index()] {
                super::super::compute::Resource::TextureTable(images) => images.as_slice(),
                _ => std::slice::from_ref(id),
            };
            for image in images {
                let index = image.index();
                if let Some(existing) = &mut self.by_resource[index] {
                    *existing |= state;
                } else {
                    self.by_resource[index] = Some(state);
                    self.touched.push(index);
                }
            }
        }
        // Only distinct resources need ordering; repeated texture-table slots
        // do not make this sort grow with the descriptor count.
        self.touched.sort_unstable();
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, D3D12_RESOURCE_STATES)> + '_ {
        self.touched
            .iter()
            .map(|&id| (id, self.by_resource[id].unwrap()))
    }
}

#[test]
fn readonly_alias_requires_union_of_descriptor_access_states() {
    let mut batch = ComputeBatch::new();
    let shared = batch.buffer(vec![0; 16]).unwrap();
    let output = batch.buffer(vec![0; 4]).unwrap();
    let totals = batch.buffer(vec![0; 4]).unwrap();
    // SAFETY: zero logical chunks and separate writable outputs; only the
    // descriptor state plan is inspected, with no memory operations executed.
    unsafe {
        batch
            .dispatch(
                "cumsum_prefix_chunks",
                &[
                    (0, shared),
                    (1, shared),
                    (2, shared),
                    (5, output),
                    (6, totals),
                ],
                [1, 1, 1],
            )
            .unwrap();
    }
    let mut states = RequiredStates::new(batch.resources().len());
    states.collect(&batch.passes()[0], batch.resources());
    assert_eq!(
        states
            .iter()
            .find(|(id, _)| *id == shared.index())
            .unwrap()
            .1,
        D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER
            | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
    );
    assert_eq!(
        states
            .iter()
            .find(|(id, _)| *id == output.index())
            .unwrap()
            .1,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS
    );
}

#[test]
fn required_states_reuses_scratch_without_retaining_previous_pass_resources() {
    let mut batch = ComputeBatch::new();
    let first = batch.buffer(vec![0; 16]).unwrap();
    let second = batch.buffer(vec![0; 16]).unwrap();
    let output = batch.buffer(vec![0; 4]).unwrap();
    let totals = batch.buffer(vec![0; 4]).unwrap();
    // SAFETY: The test inspects descriptor states only; neither pass executes.
    unsafe {
        for input in [first, second] {
            batch
                .dispatch(
                    "cumsum_prefix_chunks",
                    &[(0, input), (1, input), (2, input), (5, output), (6, totals)],
                    [1, 1, 1],
                )
                .unwrap();
        }
    }
    let mut states = RequiredStates::new(batch.resources().len());
    states.collect(&batch.passes()[0], batch.resources());
    let capacity = states.touched.capacity();
    assert!(states.iter().any(|(id, _)| id == first.index()));
    states.collect(&batch.passes()[1], batch.resources());
    assert!(states.touched.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(states.iter().any(|(id, _)| id == second.index()));
    assert!(states.iter().all(|(id, _)| id != first.index()));
    assert!(states.touched.capacity() >= capacity);
}

#[test]
fn required_states_merges_repeated_texture_table_images() {
    let mut batch = ComputeBatch::new();
    let first = batch.texture_rgba8([1, 1], vec![0; 4]).unwrap();
    let second = batch.texture_rgba8([1, 1], vec![0; 4]).unwrap();
    let shader = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|shader| shader.entry == "fine_tile_main")
        .unwrap();
    let binding = *shader
        .bindings
        .iter()
        .find(|binding| binding.kind == BindingKind::TextureTable)
        .unwrap();
    let mut images = vec![first; binding.count as usize];
    images[1] = second;
    let table = batch.texture_table(&images).unwrap();
    let pass = super::super::compute::Pass {
        initialization: None,
        shader,
        bindings: vec![(binding, table)],
        grid: [1, 1, 1],
    };
    let mut states = RequiredStates::new(batch.resources().len());
    states.collect(&pass, batch.resources());
    assert_eq!(states.touched.len(), 2);
    assert_eq!(states.touched[0], first.index());
    assert_eq!(states.touched[1], second.index());
    assert!(
        states
            .iter()
            .all(|(_, state)| state == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)
    );
    let empty = super::super::compute::Pass {
        bindings: Vec::new(),
        ..pass
    };
    states.collect(&empty, batch.resources());
    assert!(states.touched.is_empty());
}
