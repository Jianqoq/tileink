//! Descriptor writes and per-pass resource access state planning.
#[cfg(test)]
use super::super::compute::ComputeBatch;
use crate::native::shaders::{Binding, BindingKind};
use std::collections::BTreeMap;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_FORMAT_R32_TYPELESS};

pub(super) unsafe fn write(
    device: &ID3D12Device,
    binding: &Binding,
    resource: &ID3D12Resource,
    word_count: u32,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe {
        match binding.kind {
            BindingKind::Texture => device.CreateShaderResourceView(
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
                    BufferLocation: resource.GetGPUVirtualAddress(),
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

pub(super) fn required_states(
    pass: &super::super::compute::Pass,
) -> BTreeMap<usize, D3D12_RESOURCE_STATES> {
    let mut states = BTreeMap::new();
    for (binding, id) in &pass.bindings {
        let state = match binding.kind {
            BindingKind::Uniform => D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER,
            BindingKind::Read | BindingKind::Texture => {
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
            }
            BindingKind::Write | BindingKind::TextureWrite => D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        };
        // A resource may be both CBV and SRV in one pass. Preserve every read state;
        // replacing it per descriptor loses one access class (root-cause fix).
        *states
            .entry(id.index())
            .or_insert(D3D12_RESOURCE_STATE_COMMON) |= state;
    }
    states
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
    let states = required_states(&batch.passes()[0]);
    assert_eq!(
        states[&shared.index()],
        D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER
            | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
    );
    assert_eq!(
        states[&output.index()],
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS
    );
}
