//! Metal owns only API resources, encoding and completion. Scene preparation and
//! ordered workloads remain shared. Analytic fine drawing uses an Apple TBDR
//! render pass; geometry preparation and neighborhood filters use compute.
mod completion;
mod frame;
#[cfg(test)]
mod lifecycle_tests;
mod memory;
mod pipeline;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod sdf_tests;
#[cfg(test)]
mod tests;
use super::{
    Result,
    compute::ComputeBatch,
    submissions::{Pending, Ticket},
};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::*;
use std::collections::BTreeMap;
pub(super) type Object<T> = Retained<ProtocolObject<T>>;

pub struct Metal {
    pub(super) device: Object<dyn MTLDevice>,
    pub(super) queue: Object<dyn MTLCommandQueue>,
    libraries: BTreeMap<&'static str, Object<dyn MTLLibrary>>,
    pipelines: BTreeMap<&'static str, pipeline::Pipeline>,
    pending: Pending<frame::Frame>,
    failed: bool,
}

impl Metal {
    pub fn with_options(options: &crate::NativeContextOptions) -> Result<Self> {
        if options.validation && std::env::var("MTL_DEBUG_LAYER").as_deref() != Ok("1") {
            return Err(
                "Metal validation requires MTL_DEBUG_LAYER=1 before process startup".into(),
            );
        }
        let device = if let Some(identity) = &options.physical_adapter {
            MTLCopyAllDevices()
                .iter()
                .find(|device| format!("{:016x}", device.registryID()) == *identity)
                .ok_or("requested Metal registry ID unavailable")?
        } else {
            MTLCreateSystemDefaultDevice().ok_or("Metal GPU unavailable")?
        };
        let queue = device
            .newCommandQueue()
            .ok_or("Metal queue creation failed")?;
        Self::from_objects(device, queue)
    }
    pub(super) fn from_objects(
        device: Object<dyn MTLDevice>,
        queue: Object<dyn MTLCommandQueue>,
    ) -> Result<Self> {
        if !std::ptr::eq(&*queue.device(), &*device) {
            return Err("Metal queue belongs to another device".into());
        }
        if !device.supportsFamily(MTLGPUFamily::Apple7)
            || device.argumentBuffersSupport() != MTLArgumentBuffersTier::Tier2
            || device.maxThreadsPerThreadgroup().width < 256
        {
            return Err(
                "Metal requires Apple7 TBDR, argument buffer tier 2, and 256-thread groups".into(),
            );
        }
        Ok(Self {
            device,
            queue,
            libraries: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            pending: Pending::new(),
            failed: false,
        })
    }
    pub fn limits(&self) -> super::renderer::recording::Limits {
        super::renderer::recording::Limits {
            image_dimension: 16384,
            atlas_pages: 2048,
            texture_table_len: crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
            dispatch_dimension: 65535,
        }
    }
    pub fn allocate_buffer(&self, size: usize) -> Result<super::buffer::Allocation> {
        Ok(super::buffer::Allocation::Metal(memory::buffer(
            &self.device,
            size,
            MTLResourceOptions::StorageModePrivate,
        )?))
    }
    pub fn allocate_texture(
        &self,
        size: [u32; 2],
        layers: u32,
        array: bool,
    ) -> Result<super::texture::Allocation> {
        Ok(super::texture::Allocation::Metal(memory::texture(
            &self.device,
            size,
            layers,
            array,
        )?))
    }
    pub fn submit_compute(&mut self, batch: &ComputeBatch) -> Result<Ticket> {
        if self.failed {
            return Err("Metal context failed".into());
        }
        for pass in batch.passes() {
            pipeline::ensure(
                &self.device,
                &mut self.libraries,
                &mut self.pipelines,
                pass.shader,
            )?;
        }
        let frame = frame::Frame::record(self, batch)?;
        let command = frame.command.clone();
        let ticket = self.pending.track(frame)?;
        command.commit();
        self.pending.confirm(&ticket)?;
        Ok(ticket)
    }
    pub fn submit_batch(&mut self, commands: &[super::program::Dispatch]) -> Result<Ticket> {
        super::program::validate_batch(commands)?;
        let mut batch = ComputeBatch::new();
        for command in commands {
            let destination = batch.buffer(command.destination().to_vec())?;
            let source = batch.buffer(command.source().to_vec())?;
            let mut bindings = vec![(0, destination), (1, source)];
            if let Some(params) = command.params() {
                let uniform = batch.buffer(bytemuck::bytes_of(params).to_vec())?;
                bindings.push((2, uniform));
                if !matches!(command.entry(), "copy_words" | "sample_words") {
                    bindings.retain(|(slot, _)| *slot != 1);
                }
                if let Some((width, bytes)) = command.texture() {
                    let texture = batch.texture_rgba8([width, 1], bytes.to_vec())?;
                    bindings.push((3, texture));
                }
            }
            // SAFETY: the shared probe/scatter validators prove every address and
            // exclusive destination range, including rounded-up workgroup tails.
            unsafe {
                batch.dispatch(command.entry(), &bindings, [command.workgroups(), 1, 1])?;
            }
            batch.readback(destination)?;
        }
        self.submit_compute(&batch)
    }
    pub fn is_complete(&mut self, ticket: &Ticket) -> Result<bool> {
        if self.failed {
            return Err("Metal context failed".into());
        }
        let command = &self.pending.get(ticket)?.command;
        if command.status() == MTLCommandBufferStatus::Error {
            self.failed = true;
            return Err(format!("Metal command failed: {:?}", command.error()).into());
        }
        Ok(command.status() == MTLCommandBufferStatus::Completed)
    }
    pub fn readback_batch(&mut self, ticket: &Ticket) -> Result<Vec<Vec<u8>>> {
        if let Err(error) = self.pending.get(ticket)?.wait() {
            self.failed = true;
            return Err(error);
        }
        if !self.is_complete(ticket)? {
            return Err("Metal command did not complete".into());
        }
        self.pending
            .take_completed(ticket, ticket.serial())?
            .readback()
    }
    pub fn unconfirmed(&self) -> bool {
        self.failed
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn assert_valid(&self) -> Result<()> {
        if self.failed
            || self
                .pending
                .values()
                .any(|frame| frame.command.status() == MTLCommandBufferStatus::Error)
        {
            return Err("Metal command execution failed".into());
        }
        Ok(())
    }
}

impl Drop for Metal {
    fn drop(&mut self) {
        // Destruction is an explicit completion boundary. Normal submissions and
        // resize never wait for the queue; command buffers retain their own leases.
        if self.pending.values().all(|frame| frame.wait().is_ok()) {
            self.pending.clear_after_completion();
        } else {
            // An unsignaled host event or lost device must not destroy leases
            // still referenced by the GPU. Preserve them rather than guessing.
            self.pending.quarantine();
        }
    }
}
