//! Root layout and driver-specific pipeline construction/cache acceptance.
use super::*;
use std::mem::ManuallyDrop;

pub(super) fn create(
    device: &ID3D12Device,
    adapter: &IDXGIAdapter1,
    identity: &str,
    messages: &Validation,
) -> Result<(
    ID3D12RootSignature,
    BTreeMap<&'static str, ID3D12PipelineState>,
)> {
    unsafe {
        let mut parameters = [
            (D3D12_ROOT_PARAMETER_TYPE_UAV, 0),
            (D3D12_ROOT_PARAMETER_TYPE_SRV, 1),
            (D3D12_ROOT_PARAMETER_TYPE_CBV, 2),
        ]
        .into_iter()
        .map(|(ty, register)| D3D12_ROOT_PARAMETER {
            ParameterType: ty,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: register,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        })
        .collect::<Vec<_>>();
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 3,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };
        parameters.push(D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        });
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
        let serialized = serialized.unwrap();
        let bytes = std::slice::from_raw_parts(
            serialized.GetBufferPointer().cast::<u8>(),
            serialized.GetBufferSize(),
        );
        let signature: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;

        let mut pipelines = BTreeMap::new();
        for artifact in crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .filter(|a| a.format == "dxil" && a.bindings.is_empty())
        {
            let desc = adapter.GetDesc1()?;
            let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID)?;
            let identity = serde_json::to_vec(
                &serde_json::json!({"api":"dx12","vendor":desc.VendorId,"device":desc.DeviceId,"revision":desc.Revision,"subsystem":desc.SubSysId,"luid":identity,"driver":driver}),
            )?;
            let pipeline = std::cell::RefCell::new(None);
            let build = |data: &[u8]| -> windows::core::Result<(ID3D12PipelineState, Vec<u8>)> {
                let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                    pRootSignature: ManuallyDrop::new(Some(signature.clone())),
                    CS: D3D12_SHADER_BYTECODE {
                        pShaderBytecode: artifact.bytes.as_ptr().cast(),
                        BytecodeLength: artifact.bytes.len(),
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
                let pipeline = result?;
                let blob = pipeline.GetCachedBlob()?;
                let bytes = std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast::<u8>(),
                    blob.GetBufferSize(),
                )
                .to_vec();
                Ok((pipeline, bytes))
            };
            let hit = super::super::pipeline_cache::load_or_create(
                &identity,
                artifact.cache_key,
                |data| {
                    let start = messages.count();
                    match build(data) {
                        Ok((handle, _)) => {
                            *pipeline.borrow_mut() = Some(handle);
                            Ok(true)
                        }
                        Err(error) if cache_retryable(error.code()) => {
                            messages
                                .record_cache_rejection(start)
                                .map_err(|error| std::io::Error::other(error.to_string()))?;
                            Ok(false)
                        }
                        Err(error) => Err(std::io::Error::other(error)),
                    }
                },
                || {
                    let (handle, bytes) = build(&[]).map_err(std::io::Error::other)?;
                    *pipeline.borrow_mut() = Some(handle);
                    Ok(bytes)
                },
            )?;
            eprintln!(
                "native DX12 pipeline {}: {}",
                artifact.entry,
                if hit { "cache hit" } else { "compiled" }
            );
            pipelines.insert(artifact.entry, pipeline.into_inner().unwrap());
        }
        Ok((signature, pipelines))
    }
}
