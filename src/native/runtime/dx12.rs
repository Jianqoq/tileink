mod buffer_cache;
mod context;
mod debug;
mod retirement;
mod staging;
mod storage;
mod validation;
pub(super) use debug::enable_validation;
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
    adapter: IDXGIAdapter1,
    physical_identity: String,
    messages: Validation,
    queue: ID3D12CommandQueue,
    signature: Option<ID3D12RootSignature>,
    pipelines: BTreeMap<&'static str, ID3D12PipelineState>,
    fence: ID3D12Fence,
    pending: Pending<work::Work>,
    staging: buffer_cache::Pool,
    storage: buffer_cache::Pool,
    tables: Vec<compute_tables::Tables>,
    commands: Vec<command_cache::Commands>,
    compute_pipelines: BTreeMap<&'static str, compute_pipeline::Pipeline>,
    cache_identity: Vec<u8>,
}

pub struct Dx12 {
    gpu: GpuOwners,
    event: HANDLE,
    retirement: Retirement,
    #[cfg(test)]
    inject_signal_failure: bool,
}

impl Dx12 {
    pub fn allocate_buffer(&self, size: usize) -> Result<super::buffer::Allocation> {
        Ok(super::buffer::Allocation::Dx12(buffer::create(
            &self.gpu.device,
            size,
            D3D12_HEAP_TYPE_DEFAULT,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            None,
        )?))
    }
    pub fn allocate_texture(
        &self,
        size: [u32; 2],
        layers: u32,
        _array: bool,
    ) -> Result<super::texture::Allocation> {
        let resource =
            compute_texture::allocate(&self.gpu.device, size, layers, D3D12_RESOURCE_STATE_COMMON)?;
        Ok(super::texture::Allocation::Dx12(std::rc::Rc::new(
            super::texture::Dx12Allocation {
                resource,
                state: std::cell::Cell::new(D3D12_RESOURCE_STATE_COMMON),
                final_state: D3D12_RESOURCE_STATE_COMMON,
            },
        )))
    }
    pub fn validation_queue(&self) -> Validation {
        self.gpu.messages.clone()
    }

    pub fn submit_batch(&mut self, commands: &[Dispatch]) -> Result<Ticket> {
        self.retirement.wait_value()?;
        super::program::validate_batch(commands)?;
        if !commands.is_empty() && self.gpu.signature.is_none() {
            let (signature, pipelines) = pipeline::create(
                &self.gpu.device,
                &self.gpu.adapter,
                &self.gpu.physical_identity,
                &self.gpu.messages,
            )?;
            self.gpu.signature = Some(signature);
            self.gpu.pipelines = pipelines;
        }
        let frames = commands
            .iter()
            .map(|command| {
                Frame::record(
                    &self.gpu.device,
                    self.gpu
                        .signature
                        .as_ref()
                        .expect("probe pipeline initialized"),
                    &self.gpu.pipelines[command.entry()],
                    command,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        self.submit_work(work::Work::Probes(frames))
    }
    pub fn submit_compute(&mut self, batch: &super::compute::ComputeBatch) -> Result<Ticket> {
        self.retirement.wait_value()?;
        for pass in batch.passes() {
            compute_pipeline::ensure(
                &self.gpu.device,
                &self.gpu.cache_identity,
                &self.gpu.messages,
                &mut self.gpu.compute_pipelines,
                pass.shader.entry,
            )?;
        }
        let frame = compute::Frame::record(
            &self.gpu.device,
            batch,
            &self.gpu.compute_pipelines,
            &mut self.gpu.staging,
            &mut self.gpu.storage,
            &mut self.gpu.tables,
            &mut self.gpu.commands,
        )?;
        self.submit_work(work::Work::Compute(Box::new(frame)))
    }
    fn submit_work(&mut self, work: work::Work) -> Result<Ticket> {
        // Record/allocate before registering; both work types use the same
        // uncertain-submission quarantine and completion contract.
        let lists = work.lists()?;
        let synchronization = match &work {
            work::Work::Compute(frame) => frame.synchronization.clone(),
            _ => None,
        };
        let ticket = self.gpu.pending.track(work)?;
        unsafe {
            self.retirement = Retirement::Unfenced;
            if let Some(sync) = &synchronization {
                for point in &sync.waits {
                    self.gpu.queue.Wait(&point.fence, point.value)?;
                }
            }
            self.gpu.queue.ExecuteCommandLists(&lists);
            #[cfg(test)]
            if std::mem::take(&mut self.inject_signal_failure) {
                return Err("injected Signal failure after Execute".into());
            }
            if let Some(sync) = &synchronization {
                for point in &sync.signals {
                    self.gpu.queue.Signal(&point.fence, point.value)?;
                }
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
    pub fn submit(&mut self, command: &super::program::Probe) -> Result<Ticket> {
        self.submit_batch(&[command.clone().into()])
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
        let mut work = self.gpu.pending.take_completed(ticket, completed)?;
        let result = work.readback();
        // Never recycle on Drop or uncertain submission: only a confirmed fence
        // permits overwriting upload memory that belonged to an earlier frame.
        if let work::Work::Compute(frame) = &mut work {
            self.gpu.staging.retire(std::mem::take(&mut frame.uploads));
            self.gpu.storage.retire(std::mem::take(&mut frame.storage));
            self.gpu.commands.push(frame.command_owners());
            // Descriptor writes may overwrite an older frame only after this
            // confirmed fence completion, never on Drop or uncertain submission.
            if let Some(tables) = frame.tables.take()
                && !tables.is_empty()
            {
                self.gpu.tables.push(tables);
            }
        }
        result
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
    pub fn execute(&mut self, case: &super::program::Probe) -> Result<Vec<u8>> {
        let ticket = self.submit(case)?;
        self.readback(&ticket)
    }

    pub fn pending_count(&self) -> usize {
        self.gpu.pending.len()
    }

    pub fn is_complete(&mut self, ticket: &Ticket) -> Result<bool> {
        self.gpu.pending.get(ticket)?;
        self.retirement.wait_value()?;
        let observed = unsafe { self.gpu.fence.GetCompletedValue() };
        if observed == u64::MAX {
            self.retirement = Retirement::Failed;
            return Err("DX12 device removed while polling completion".into());
        }
        Ok(self.gpu.pending.is_completed(ticket, observed)?)
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
            // Reporting is optional; preserving unknown in-flight owners is not.
            // This fixes teardown panics when ordinary contexts have no info queue.
            if let Some(queue) = &self.gpu.messages.queue {
                unsafe {
                    let _ = queue.AddMessage(
                    D3D12_MESSAGE_CATEGORY_EXECUTION,
                    D3D12_MESSAGE_SEVERITY_ERROR,
                    D3D12_MESSAGE_ID_UNKNOWN,
                    windows::core::s!(
                        "native DX12 cleanup could not confirm completion; resources quarantined"
                    ),
                );
                }
            }
            // Unknown in-flight work cannot be freed safely or waited on a fabricated fence. Quarantine
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

#[path = "dx12/buffer.rs"]
mod buffer;
#[path = "dx12/compute.rs"]
mod compute;
mod compute_bindings;
#[path = "dx12/compute_pipeline.rs"]
mod compute_pipeline;
mod compute_resources;
#[path = "dx12/work.rs"]
mod work;

#[path = "dx12/compute_texture.rs"]
mod compute_texture;

#[path = "dx12/compute_tables.rs"]
mod compute_tables;

mod compute_copy;

#[cfg(test)]
impl Dx12 {
    pub(crate) fn import_descriptor(&self) -> crate::native::interop::dx12::ContextDescriptor {
        crate::native::interop::dx12::ContextDescriptor {
            device: self.gpu.device.clone(),
            queue: self.gpu.queue.clone(),
            validation: true,
        }
    }
}

#[path = "dx12/import.rs"]
mod import;

#[path = "dx12/synchronization.rs"]
mod synchronization;

#[cfg(test)]
impl Dx12 {
    pub(crate) fn inject_signal_failure_for_test(&mut self) {
        self.inject_signal_failure = true;
    }
}

#[cfg(test)]
mod command_cache_tests;

mod command_cache;
