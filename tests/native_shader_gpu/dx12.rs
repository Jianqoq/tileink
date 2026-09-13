#[path = "dx12_validation.rs"]
mod validation;
use validation::cache_retryable;
pub use validation::{Validation, assert_valid};
// Native D3D12 execution of the same compiled probe contract.
use super::{Result, cases::Case, retirement::Retirement};
use std::{collections::BTreeMap, mem::ManuallyDrop};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::*,
            Direct3D12::*,
            Dxgi::{Common::*, *},
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::Interface,
};

// Keep every GPU owner together. Error-path retention clones this complete
// aggregate, so adding a fence/resource cannot silently omit it from quarantine.
#[derive(Clone)]
struct GpuOwners {
    device: ID3D12Device,
    messages: Validation,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    signature: ID3D12RootSignature,
    pipelines: BTreeMap<&'static str, ID3D12PipelineState>,
    fence: ID3D12Fence,
    buffers: Vec<ID3D12Resource>,
}

pub struct Dx12 {
    gpu: GpuOwners,
    event: HANDLE,
    submitted: u64,
    retirement: Retirement,
}

impl Dx12 {
    pub fn new(identity: &str) -> Result<Self> {
        unsafe {
            let mut debug = None;
            D3D12GetDebugInterface(&mut debug)?;
            let debug: ID3D12Debug = debug.unwrap();
            debug.EnableDebugLayer();
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let mut selected = None;
            for index in 0.. {
                let adapter = match factory.EnumAdapters1(index) {
                    Ok(a) => a,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e.into()),
                };
                let desc = adapter.GetDesc1()?;
                let bytes = [
                    desc.AdapterLuid.LowPart.to_le_bytes(),
                    desc.AdapterLuid.HighPart.to_le_bytes(),
                ]
                .concat();
                let actual: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                if actual == identity {
                    selected = Some(adapter);
                    break;
                }
            }
            let adapter = selected.ok_or("requested native DX12 physical GPU unavailable")?;
            let mut device = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device)?;
            let device: ID3D12Device = device.unwrap();
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })?;
            let allocator: ID3D12CommandAllocator =
                device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            list.Close()?;
            let parameters = [
                (D3D12_ROOT_PARAMETER_TYPE_UAV, 0),
                (D3D12_ROOT_PARAMETER_TYPE_SRV, 1),
                (D3D12_ROOT_PARAMETER_TYPE_CBV, 2),
            ]
            .map(|(ty, register)| D3D12_ROOT_PARAMETER {
                ParameterType: ty,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: register,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            let desc = D3D12_ROOT_SIGNATURE_DESC {
                NumParameters: 3,
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
            let messages = Validation::new(device.cast()?);
            let mut pipelines = BTreeMap::new();
            for artifact in tileink::NATIVE_SHADER_ARTIFACTS
                .iter()
                .filter(|a| a.format == "dxil")
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
                let hit = super::pipeline_cache::load_or_create(
                    &identity,
                    artifact.cache_key,
                    |data| {
                        let start = messages.queue.GetNumStoredMessages();
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
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let event = CreateEventW(None, false, false, None)?;
            Ok(Self {
                gpu: GpuOwners {
                    device,
                    messages,
                    queue,
                    allocator,
                    list,
                    signature,
                    pipelines,
                    fence,
                    buffers: Vec::new(),
                },
                event,
                submitted: 0,
                retirement: Retirement::Idle,
            })
        }
    }

    pub fn validation_queue(&self) -> Validation {
        self.gpu.messages.clone()
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
            self.gpu.device.CreateCommittedResource(
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
            self.gpu.buffers.push(resource.clone());
            Ok(resource)
        }
    }

    pub fn execute(&mut self, case: &Case) -> Result<Vec<u8>> {
        unsafe {
            self.wait()?;
            self.gpu.buffers.clear();
            let size = case.destination.len();
            let destination = self.buffer(
                size,
                D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                None,
            )?;
            let initial = self.buffer(
                size,
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(&case.destination),
            )?;
            let source = self.buffer(
                case.source.len(),
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(&case.source),
            )?;
            let params = self.buffer(
                256,
                D3D12_HEAP_TYPE_UPLOAD,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                D3D12_RESOURCE_FLAG_NONE,
                Some(bytemuck::bytes_of(&case.params)),
            )?;
            let readback = self.buffer(
                size,
                D3D12_HEAP_TYPE_READBACK,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_FLAG_NONE,
                None,
            )?;
            self.gpu.allocator.Reset()?;
            self.gpu.list.Reset(&self.gpu.allocator, None)?;
            // Default-heap buffers start COMMON; an explicit transition avoids
            // relying on the initial state ignored by current D3D12 runtimes.
            self.transition(
                &destination,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            self.gpu
                .list
                .CopyBufferRegion(&destination, 0, &initial, 0, size as u64);
            self.transition(
                &destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            );
            self.gpu.list.SetComputeRootSignature(&self.gpu.signature);
            self.gpu
                .list
                .SetPipelineState(&self.gpu.pipelines[case.entry]);
            self.gpu
                .list
                .SetComputeRootUnorderedAccessView(0, destination.GetGPUVirtualAddress());
            self.gpu
                .list
                .SetComputeRootShaderResourceView(1, source.GetGPUVirtualAddress());
            self.gpu
                .list
                .SetComputeRootConstantBufferView(2, params.GetGPUVirtualAddress());
            self.gpu
                .list
                .Dispatch(case.params.count.div_ceil(64).max(1), 1, 1);
            self.transition(
                &destination,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            self.gpu
                .list
                .CopyBufferRegion(&readback, 0, &destination, 0, size as u64);
            self.gpu.list.Close()?;
            let list: ID3D12CommandList = self.gpu.list.cast()?;
            self.retirement = Retirement::Unfenced;
            self.gpu.queue.ExecuteCommandLists(&[Some(list)]);
            self.submitted += 1;
            self.gpu.queue.Signal(&self.gpu.fence, self.submitted)?;
            self.retirement = Retirement::Signaled(self.submitted);
            self.wait()?;
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
            self.gpu
                .list
                .ResourceBarrier(std::slice::from_ref(&barrier));
            ManuallyDrop::drop(&mut (*barrier.Anonymous.Transition).pResource);
        }
    }

    fn wait(&mut self) -> Result<()> {
        let Some(value) = self.retirement.wait_value()? else {
            return Ok(());
        };
        unsafe {
            if self.gpu.fence.GetCompletedValue() == u64::MAX {
                self.retirement = Retirement::Failed;
                return Err("DX12 device removed".into());
            }
            self.retirement = Retirement::Failed;
            if self.gpu.fence.GetCompletedValue() < value {
                self.gpu.fence.SetEventOnCompletion(value, self.event)?;
                if WaitForSingleObject(self.event, 30_000) != WAIT_OBJECT_0 {
                    return Err("DX12 fence wait failed".into());
                }
            }
            self.gpu.device.GetDeviceRemovedReason()?;
            self.retirement = Retirement::Idle;
            Ok(())
        }
    }
}

impl Drop for Dx12 {
    fn drop(&mut self) {
        if self.wait().is_err() {
            // This is a disposable verification device. Unknown in-flight work
            // cannot be freed safely or waited on a fabricated fence. Quarantine
            // all owners until process teardown, preserving the original error.
            std::mem::forget(self.gpu.clone());
            return;
        }
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn failed_retirement_retains_fence_owner_until_process_exit() -> Result<()> {
    if isolated_fault_test("dx12::failed_retirement_retains_fence_owner_until_process_exit")? {
        return Ok(());
    }
    // COM reference counts are used only in this isolated lifetime regression.
    // The observer keeps querying safe; comparing before/after detects whether
    // the cleanup path released its own fence reference despite that observer.
    fn references(fence: &windows::core::IUnknown) -> u32 {
        unsafe {
            let count = (fence.vtable().AddRef)(fence.as_raw());
            (fence.vtable().Release)(fence.as_raw());
            count - 1
        }
    }
    for (state, released) in [
        (Retirement::Idle, 1),
        (Retirement::Unfenced, 0),
        (Retirement::Failed, 0),
    ] {
        let mut context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
        let observer: windows::core::IUnknown = context.gpu.fence.cast()?;
        let before = references(&observer);
        context.retirement = state;
        drop(context);
        assert_eq!(references(&observer), before - released);
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_cache_diagnostics_do_not_hide_other_attempts() -> Result<()> {
    if isolated_fault_test("dx12::rejected_cache_diagnostics_do_not_hide_other_attempts")? {
        return Ok(());
    }
    let context = Dx12::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let report = context.validation_queue();
    unsafe {
        let start = report.queue.GetNumStoredMessages();
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_STATE_CREATION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB,
            windows::core::s!("expected cache test diagnostic"),
        )?;
        report.record_cache_rejection(start)?;
        assert_valid(&report)?;
        report.queue.AddMessage(
            D3D12_MESSAGE_CATEGORY_STATE_CREATION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB,
            windows::core::s!("outside rejected attempt"),
        )?;
        assert!(assert_valid(&report).is_err());
    }
    Ok(())
}

fn isolated_fault_test(name: &str) -> Result<bool> {
    if std::env::var("TILEINK_NATIVE_FAULT_WORKER").ok().as_deref() == Some(name) {
        return Ok(false);
    }
    // D3D12 may share device/InfoQueue objects between contexts. Deliberate
    // diagnostic injection and quarantined owners must die with a child process
    // before ordinary validation runs, without clearing any validation messages.
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            name,
            "--test-threads=1",
            "--nocapture",
        ])
        .env("TILEINK_NATIVE_FAULT_WORKER", name)
        .status()?;
    if !status.success() {
        return Err(format!("isolated GPU fault test failed: {name}: {status}").into());
    }
    Ok(true)
}
