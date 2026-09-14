use super::{
    Result,
    validation::{Validation, cache_retryable},
};
use crate::native::shaders::{BindingKind, NativeShaderArtifact};
use std::{collections::BTreeMap, mem::ManuallyDrop};
use windows::{
    Win32::Graphics::{Direct3D12::*, Dxgi::*},
    core::Interface,
};
#[derive(Clone)]
pub struct Pipeline {
    pub signature: ID3D12RootSignature,
    pub state: ID3D12PipelineState,
    pub resources: Option<u32>,
    pub samplers: Option<u32>,
    pub grid: Option<u32>,
}
pub fn identity(adapter: &IDXGIAdapter1, luid: &str) -> Result<Vec<u8>> {
    unsafe {
        let d = adapter.GetDesc1()?;
        let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID)?;
        Ok(serde_json::to_vec(
            &serde_json::json!({"api":"dx12","vendor":d.VendorId,"device":d.DeviceId,"revision":d.Revision,"subsystem":d.SubSysId,"luid":luid,"driver":driver}),
        )?)
    }
}
pub fn ensure(
    device: &ID3D12Device,
    identity: &[u8],
    messages: &Validation,
    pipelines: &mut BTreeMap<&'static str, Pipeline>,
    entry: &'static str,
) -> Result<()> {
    if pipelines.contains_key(entry) {
        return Ok(());
    }
    let shader = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.format == "dxil" && a.entry == entry && !a.bindings.is_empty())
        .ok_or("native DX12 compute shader unavailable")?;
    pipelines.insert(entry, create(device, identity, messages, shader)?);
    Ok(())
}
fn create(
    device: &ID3D12Device,
    identity: &[u8],
    messages: &Validation,
    shader: &NativeShaderArtifact,
) -> Result<Pipeline> {
    unsafe {
        let table = |sampler: bool| {
            shader
                .bindings
                .iter()
                .filter(|b| !b.internal && (b.kind == BindingKind::Sampler) == sampler)
                .enumerate()
                .map(|(index, b)| D3D12_DESCRIPTOR_RANGE {
                    RangeType: match b.kind {
                        BindingKind::Sampler => D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
                        BindingKind::Uniform => D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
                        BindingKind::Read | BindingKind::Texture | BindingKind::TextureArray => {
                            D3D12_DESCRIPTOR_RANGE_TYPE_SRV
                        }
                        BindingKind::Write | BindingKind::TextureWrite => {
                            D3D12_DESCRIPTOR_RANGE_TYPE_UAV
                        }
                    },
                    NumDescriptors: 1,
                    BaseShaderRegister: b.slot,
                    RegisterSpace: 0,
                    OffsetInDescriptorsFromTableStart: index as u32,
                })
                .collect::<Vec<_>>()
        };
        let resource_ranges = table(false);
        let sampler_ranges = table(true);
        let mut parameters = Vec::new();
        let mut add_table = |ranges: &[D3D12_DESCRIPTOR_RANGE]| {
            if ranges.is_empty() {
                return None;
            }
            let index = parameters.len() as u32;
            parameters.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: ranges.len() as u32,
                        pDescriptorRanges: ranges.as_ptr(),
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            Some(index)
        };
        let resources = add_table(&resource_ranges);
        let samplers = add_table(&sampler_ranges);
        let grid = if shader.bindings.iter().any(|b| b.internal) {
            let index = parameters.len() as u32;
            parameters.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 31,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            Some(index)
        } else {
            None
        };
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: parameters.len() as u32,
            pParameters: parameters.as_ptr(),
            ..Default::default()
        };
        let mut serialized = None;
        let mut errors = None;
        D3D12SerializeRootSignature(
            &desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut serialized,
            Some(&mut errors),
        )?;
        let blob = serialized.unwrap();
        let bytes =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize());
        let signature: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pipeline = std::cell::RefCell::new(None);
        let build = |data: &[u8]| -> windows::core::Result<(ID3D12PipelineState, Vec<u8>)> {
            let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: ManuallyDrop::new(Some(signature.clone())),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: shader.bytes.as_ptr().cast(),
                    BytecodeLength: shader.bytes.len(),
                },
                CachedPSO: D3D12_CACHED_PIPELINE_STATE {
                    pCachedBlob: if data.is_empty() {
                        std::ptr::null()
                    } else {
                        data.as_ptr().cast()
                    },
                    CachedBlobSizeInBytes: data.len(),
                },
                ..Default::default()
            };
            let result = device.CreateComputePipelineState::<ID3D12PipelineState>(&desc);
            ManuallyDrop::drop(&mut desc.pRootSignature);
            let state = result?;
            let blob = state.GetCachedBlob()?;
            Ok((
                state,
                std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize())
                    .to_vec(),
            ))
        };
        let hit = super::super::pipeline_cache::load_or_create_for_layout(
            identity,
            shader.cache_key,
            if samplers.is_some() {
                b"native-compute-sampler-table-v1"
            } else {
                b"native-compute-buffer-table-v1"
            },
            |bytes| {
                let start = messages.queue.GetNumStoredMessages();
                match build(bytes) {
                    Ok((state, _)) => {
                        *pipeline.borrow_mut() = Some(state);
                        Ok(true)
                    }
                    Err(e) if cache_retryable(e.code()) => {
                        messages
                            .record_cache_rejection(start)
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                        Ok(false)
                    }
                    Err(e) => Err(std::io::Error::other(e)),
                }
            },
            || {
                let (state, bytes) = build(&[]).map_err(std::io::Error::other)?;
                *pipeline.borrow_mut() = Some(state);
                Ok(bytes)
            },
        )?;
        eprintln!(
            "native DX12 compute {}: {}",
            shader.entry,
            if hit { "cache hit" } else { "compiled" }
        );
        Ok(Pipeline {
            signature,
            state: pipeline.into_inner().unwrap(),
            resources,
            samplers,
            grid,
        })
    }
}
