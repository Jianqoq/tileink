use super::{
    Result,
    validation::{Validation, cache_retryable},
};
use crate::native::shaders::{BindingKind, ShaderArtifact};
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
    // SAFETY: The retained adapter is live and all descriptor/driver output storage remains valid during these queries.
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
    pipelines: &mut BTreeMap<String, Pipeline>,
    pass: &crate::native::runtime::compute::Pass,
) -> Result<()> {
    let entry = pass.entry();
    if pipelines.contains_key(pass.key()) {
        return Ok(());
    }
    let shader = if pass.native_source.is_some() {
        pass.artifact()
    } else {
        crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .find(|a| a.format == "dxil" && a.entry == entry && !a.bindings.is_empty())
            .ok_or("native DX12 compute shader unavailable")?
            .to_owned()
    };
    let compiled = pass
        .native_source
        .as_ref()
        .map(|source| crate::native::runtime::program_shader::dxc::compile(source, "dxil"))
        .transpose()?;
    let (bytes, key) = compiled
        .as_ref()
        .map_or((shader.bytes, shader.cache_key), |s| {
            (s.bytes.as_slice(), s.key.as_str())
        });
    pipelines.insert(
        pass.key().to_owned(),
        create(device, identity, messages, &shader, bytes, key)?,
    );
    Ok(())
}
#[allow(
    clippy::expect_used,
    reason = "Successful cache lookup or compilation installs the pipeline before returning"
)]
fn create(
    device: &ID3D12Device,
    identity: &[u8],
    messages: &Validation,
    shader: &ShaderArtifact<'_>,
    shader_bytes: &[u8],
    shader_key: &str,
) -> Result<Pipeline> {
    // SAFETY: Validated reflection bounds descriptor ranges and root slots; shader and descriptor arrays remain live until synchronous pipeline creation returns.
    unsafe {
        let table = |sampler: bool| -> Result<Vec<D3D12_DESCRIPTOR_RANGE>> {
            let mut ranges = Vec::new();
            let mut offset = 0u32;
            for b in shader
                .bindings
                .iter()
                .filter(|b| !b.internal && (b.kind == BindingKind::Sampler) == sampler)
            {
                ranges.push(D3D12_DESCRIPTOR_RANGE {
                    RangeType: match b.kind {
                        BindingKind::Sampler => D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
                        BindingKind::Uniform => D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
                        BindingKind::Read
                        | BindingKind::Texture
                        | BindingKind::TextureArray
                        | BindingKind::TextureTable => D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                        BindingKind::Write
                        | BindingKind::TextureWrite
                        | BindingKind::TextureReadWrite => D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                    },
                    NumDescriptors: b.count,
                    BaseShaderRegister: b.slot,
                    RegisterSpace: 0,
                    OffsetInDescriptorsFromTableStart: offset,
                });
                offset = offset
                    .checked_add(b.count)
                    .ok_or("native root descriptor count overflow")?;
            }
            Ok(ranges)
        };
        let resource_ranges = table(false)?;
        let sampler_ranges = table(true)?;
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
        let grid = shader.bindings.iter().any(|b| b.internal).then(|| {
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
            index
        });
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: parameters.len() as u32,
            pParameters: parameters.as_ptr(),
            ..Default::default()
        };
        let mut serialized = None;
        let mut errors = None;
        D3D12SerializeRootSignature(
            &raw const desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &raw mut serialized,
            Some(&raw mut errors),
        )?;
        let blob = serialized.ok_or("DX12 root-signature serialization returned no blob")?;
        let bytes =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize());
        let signature: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pipeline = std::cell::RefCell::new(None);
        let build = |data: &[u8]| -> windows::core::Result<(ID3D12PipelineState, Vec<u8>)> {
            let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: ManuallyDrop::new(Some(signature.clone())),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: shader_bytes.as_ptr().cast(),
                    BytecodeLength: shader_bytes.len(),
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
            let result = device.CreateComputePipelineState::<ID3D12PipelineState>(&raw const desc);
            ManuallyDrop::drop(&mut desc.pRootSignature);
            let state = result?;
            let blob = state.GetCachedBlob()?;
            Ok((
                state,
                std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize())
                    .to_vec(),
            ))
        };
        super::super::pipeline_cache::load_or_create_for_layout(
            identity,
            shader_key,
            if samplers.is_some() {
                b"native-compute-sampler-table-v1"
            } else {
                b"native-compute-buffer-table-v1"
            },
            |bytes| {
                let start = messages.count();
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
        Ok(Pipeline {
            signature,
            state: pipeline
                .into_inner()
                .expect("Successful cache lookup or compilation installs the pipeline"),
            resources,
            samplers,
            grid,
        })
    }
}
