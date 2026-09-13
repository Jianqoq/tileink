#[path = "dx12_validation.rs"]
mod validation;
use validation::cache_retryable;
#[path = "dx12_frame.rs"]
mod frame;
use super::submissions::{Pending, Ticket};
use frame::Frame;
pub use validation::{Validation, assert_valid};
// Native D3D12 execution of the same compiled probe contract.
use super::{Result, cases::Case, retirement::Retirement};
use std::{collections::BTreeMap, mem::ManuallyDrop};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Graphics::{Direct3D::*, Direct3D12::*, Dxgi::*},
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
    signature: ID3D12RootSignature,
    pipelines: BTreeMap<&'static str, ID3D12PipelineState>,
    fence: ID3D12Fence,
    pending: Pending<Frame>,
}

pub struct Dx12 {
    gpu: GpuOwners,
    event: HANDLE,
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
                    signature,
                    pipelines,
                    fence,
                    pending: Pending::new(),
                },
                event,
                retirement: Retirement::Idle,
            })
        }
    }

    pub fn validation_queue(&self) -> Validation {
        self.gpu.messages.clone()
    }

    pub fn submit(&mut self, case: &Case) -> Result<Ticket> {
        self.retirement.wait_value()?; // Reject a previously unconfirmed/failed attempt; do not wait.
        let frame = Frame::record(
            &self.gpu.device,
            &self.gpu.signature,
            &self.gpu.pipelines[case.entry],
            case,
        )?;
        let ticket = self.gpu.pending.track(frame)?;
        unsafe {
            let list: ID3D12CommandList = self.gpu.pending.get(&ticket)?.list.cast()?;
            self.retirement = Retirement::Unfenced;
            self.gpu.queue.ExecuteCommandLists(&[Some(list)]);
            self.gpu.queue.Signal(&self.gpu.fence, ticket.serial())?;
        }
        self.gpu.pending.confirm(&ticket)?;
        self.retirement = Retirement::Signaled(ticket.serial());
        Ok(ticket)
    }

    pub fn readback(&mut self, ticket: &Ticket) -> Result<Vec<u8>> {
        self.gpu.pending.get(ticket)?; // Validate device identity before touching its fence.
        let latest = self.retirement.wait_value()?;
        let completed = match self.wait_until(ticket.serial()) {
            Ok(value) => value,
            Err(error) => {
                self.retirement = Retirement::Failed;
                return Err(error);
            }
        };
        if latest.is_some_and(|value| completed >= value) {
            self.retirement = Retirement::Idle;
        }
        self.gpu
            .pending
            .take_completed(ticket, completed)?
            .readback()
    }

    pub fn execute(&mut self, case: &Case) -> Result<Vec<u8>> {
        let ticket = self.submit(case)?;
        self.readback(&ticket)
    }

    pub fn pending_count(&self) -> usize {
        self.gpu.pending.len()
    }

    fn wait_until(&self, value: u64) -> Result<u64> {
        unsafe {
            if self.gpu.fence.GetCompletedValue() == u64::MAX {
                return Err("DX12 device removed".into());
            }
            if self.gpu.fence.GetCompletedValue() < value {
                self.gpu.fence.SetEventOnCompletion(value, self.event)?;
                if WaitForSingleObject(self.event, 30_000) != WAIT_OBJECT_0 {
                    return Err("DX12 fence wait failed".into());
                }
            }
            self.gpu.device.GetDeviceRemovedReason()?;
            let completed = self.gpu.fence.GetCompletedValue();
            if completed == u64::MAX || completed < value {
                return Err("DX12 invalid completion".into());
            }
            Ok(completed)
        }
    }

    fn wait(&mut self) -> Result<()> {
        if let Some(value) = self.retirement.wait_value()? {
            self.retirement = Retirement::Failed;
            self.wait_until(value)?;
            self.retirement = Retirement::Idle;
        }
        Ok(())
    }
}

impl Drop for Dx12 {
    fn drop(&mut self) {
        if let Err(error) = self.wait() {
            eprintln!("native DX12 cleanup could not confirm completion: {error}");
            unsafe {
                let _ = self.gpu.messages.queue.AddMessage(
                    D3D12_MESSAGE_CATEGORY_EXECUTION,
                    D3D12_MESSAGE_SEVERITY_ERROR,
                    D3D12_MESSAGE_ID_UNKNOWN,
                    windows::core::s!(
                        "native DX12 cleanup could not confirm completion; resources quarantined"
                    ),
                );
            }
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
    if super::isolation::run("dx12::failed_retirement_retains_fence_owner_until_process_exit")? {
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
        let report = context.validation_queue();
        let message_count = unsafe { report.queue.GetNumStoredMessages() };
        context.retirement = state;
        drop(context);
        if released == 0 {
            assert!(
                unsafe { report.queue.GetNumStoredMessages() } > message_count,
                "failed teardown must be observable"
            );
        }
        assert_eq!(references(&observer), before - released);
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_cache_diagnostics_do_not_hide_other_attempts() -> Result<()> {
    if super::isolation::run("dx12::rejected_cache_diagnostics_do_not_hide_other_attempts")? {
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
