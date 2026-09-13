mod retirement;
mod validation;
use validation::cache_retryable;
mod frame;
mod pipeline;
use super::submissions::{Pending, Ticket};
use frame::Frame;
pub use validation::{Validation, assert_valid};
// Native D3D12 execution of the same compiled probe contract.
use super::{Result, program::Dispatch};
use retirement::Retirement;
use std::collections::BTreeMap;
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
    pending: Pending<Vec<Frame>>,
}

pub struct Dx12 {
    gpu: GpuOwners,
    event: HANDLE,
    retirement: Retirement,
    #[cfg(test)]
    inject_signal_failure: bool,
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
            let messages = Validation::new(device.cast()?);
            let (signature, pipelines) = pipeline::create(&device, &adapter, identity, &messages)?;
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
                #[cfg(test)]
                inject_signal_failure: false,
            })
        }
    }

    pub fn validation_queue(&self) -> Validation {
        self.gpu.messages.clone()
    }

    pub fn submit_batch(&mut self, commands: &[Dispatch]) -> Result<Ticket> {
        self.retirement.wait_value()?;
        super::program::validate_batch(commands)?;
        let frames = commands
            .iter()
            .map(|command| {
                Frame::record(
                    &self.gpu.device,
                    &self.gpu.signature,
                    &self.gpu.pipelines[command.entry],
                    command,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        // Cast/allocate before registering the attempted submission. Every list
        // and its resources remain retained even if Signal fails after Execute.
        let lists = frames
            .iter()
            .map(|frame| frame.list.cast().map(Some))
            .collect::<windows::core::Result<Vec<Option<ID3D12CommandList>>>>()?;
        let ticket = self.gpu.pending.track(frames)?;
        unsafe {
            self.retirement = Retirement::Unfenced;
            self.gpu.queue.ExecuteCommandLists(&lists);
            #[cfg(test)]
            if std::mem::take(&mut self.inject_signal_failure) {
                return Err("injected Signal failure after Execute".into());
            }
            self.gpu.queue.Signal(&self.gpu.fence, ticket.serial())?;
        }
        self.gpu.pending.confirm(&ticket)?;
        self.retirement = Retirement::Signaled(ticket.serial());
        Ok(ticket)
    }

    pub fn unconfirmed(&self) -> bool {
        matches!(self.retirement, Retirement::Unfenced | Retirement::Failed)
    }

    #[cfg(test)]
    pub fn submit(&mut self, command: &Dispatch) -> Result<Ticket> {
        self.submit_batch(std::slice::from_ref(command))
    }

    pub fn readback_batch(&mut self, ticket: &Ticket) -> Result<Vec<Vec<u8>>> {
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
            .iter()
            .map(Frame::readback)
            .collect()
    }

    #[cfg(test)]
    pub fn readback(&mut self, ticket: &Ticket) -> Result<Vec<u8>> {
        let mut outputs = self.readback_batch(ticket)?;
        if outputs.len() != 1 {
            return Err("expected a single dispatch".into());
        }
        Ok(outputs.remove(0))
    }

    #[cfg(test)]
    pub fn execute(&mut self, case: &Dispatch) -> Result<Vec<u8>> {
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

#[cfg(test)]
mod tests;
