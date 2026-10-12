mod compute;
pub(super) mod compute_memory;
mod compute_pipeline;
mod context;
mod limits;
mod staging;
mod upload;
mod validation;
use super::submissions::{Pending, Ticket};
// Native Vulkan execution for renderer and conformance workloads.
use super::{Result, program::Dispatch};
use ash::{Entry, vk};
use std::collections::BTreeMap;

pub struct Vulkan {
    host_owner: Option<std::rc::Rc<dyn std::any::Any>>,
    entry: Entry,
    debug: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    instance: ash::Instance,
    #[cfg(test)]
    physical: vk::PhysicalDevice,
    device: ash::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    pending: Pending<compute::Frame>,
    frame_cache: frame_cache::Cache,
    properties: vk::PhysicalDeviceProperties,
    compute_pipelines: BTreeMap<String, compute_pipeline::Pipeline>,
    family: u32,
    failed: bool,
    texture_tables: bool,
    timeline_semaphores: bool,
    families: Vec<vk::QueueFamilyProperties>,
    #[cfg(test)]
    injected_submit_error: Option<vk::Result>,
}

impl Vulkan {
    pub fn allocate_buffer(&self, size: usize) -> Result<super::buffer::Allocation> {
        Ok(super::buffer::Allocation::Vulkan(std::rc::Rc::new(
            compute_memory::Arena::new(
                &self.device,
                &self.memory,
                &[size as u64],
                vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::UNIFORM_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )?,
        )))
    }
    pub fn import_texture(
        &self,
        descriptor: crate::native::interop::vulkan::TextureDescriptor,
    ) -> Result<super::texture::Allocation> {
        if descriptor
            .size
            .iter()
            .any(|&n| n == 0 || n > self.properties.limits.max_image_dimension2_d)
        {
            return Err("invalid imported Vulkan image extent".into());
        }
        Ok(super::texture::Allocation::Vulkan(std::rc::Rc::new(
            compute_texture::Image::import(&std::rc::Rc::new(self.device.clone()), descriptor)?,
        )))
    }
    pub fn allocate_texture(
        &self,
        size: [u32; 2],
        layers: u32,
        array: bool,
        format: crate::ShaderTextureFormat,
    ) -> Result<super::texture::Allocation> {
        let descriptor = super::compute::Texture {
            size,
            format,
            layers,
            array,
            bytes: Vec::new().into(),
            persistent: None,
        };
        Ok(super::texture::Allocation::Vulkan(std::rc::Rc::new(
            compute_texture::Image::new(
                &std::rc::Rc::new(self.device.clone()),
                &self.memory,
                &descriptor,
            )?,
        )))
    }
    pub fn validation_messages(&self) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::clone(&self.messages)
    }

    pub fn submit_batch(&mut self, commands: &[Dispatch]) -> Result<Ticket> {
        self.submit_compute(&super::program::record_batch(commands)?)
    }
    pub fn submit_compute(&mut self, batch: &super::compute::ComputeBatch) -> Result<Ticket> {
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        if let Some((_, sync)) = &batch.synchronization {
            match sync {
                crate::native::interop::Synchronization::Vulkan(sync) => synchronization::validate(
                    sync,
                    self.family,
                    &self.families,
                    self.timeline_semaphores,
                )?,
                #[cfg(feature = "dx12")]
                _ => return Err("DX12 synchronization requires a DX12 context".into()),
            }
        }
        if self.pending.len() == 0
            && self
                .compute_pipelines
                .keys()
                .filter(|k| k.starts_with("program:"))
                .count()
                >= 128
        {
            self.compute_pipelines
                .retain(|key, _| !key.starts_with("program:"));
        }
        for pass in batch.passes() {
            if !self.texture_tables
                && pass
                    .shader
                    .bindings
                    .iter()
                    .any(|b| b.kind == crate::native::shaders::BindingKind::TextureTable)
            {
                return Err("native Vulkan nonuniform texture indexing is unavailable".into());
            }
            compute_pipeline::ensure(
                &self.device,
                &self.properties,
                &mut self.compute_pipelines,
                pass,
            )?;
        }
        let frame = compute::Frame::record(
            &self.device,
            &self.memory,
            &self.properties,
            self.family,
            batch,
            &self.compute_pipelines,
            &mut self.frame_cache,
        )?;
        self.submit_work(frame)
    }
    fn submit_work(&mut self, work: compute::Frame) -> Result<Ticket> {
        let buffers = [work.commands.command];
        let synchronization = work.synchronization.clone();
        let waits: Vec<_> = synchronization
            .iter()
            .flat_map(|sync| sync.waits.iter().map(|wait| wait.semaphore.handle()))
            .collect();
        let stages: Vec<_> = synchronization
            .iter()
            .flat_map(|sync| {
                sync.waits
                    .iter()
                    .map(|_| vk::PipelineStageFlags::ALL_COMMANDS)
            })
            .collect();
        let signals: Vec<_> = synchronization
            .iter()
            .flat_map(|sync| sync.signals.iter().map(|signal| signal.handle()))
            .collect();
        let wait_values: Vec<_> = synchronization
            .iter()
            .flat_map(|sync| sync.waits.iter().map(|wait| wait.semaphore.value()))
            .collect();
        let signal_values: Vec<_> = synchronization
            .iter()
            .flat_map(|sync| sync.signals.iter().map(|signal| signal.value()))
            .collect();
        let timeline = synchronization.iter().any(|sync| {
            sync.waits
                .iter()
                .map(|wait| wait.semaphore)
                .chain(sync.signals.iter().copied())
                .any(|point| {
                    matches!(
                        point,
                        crate::native::interop::vulkan::SemaphorePoint::Timeline { .. }
                    )
                })
        });
        let mut values = vk::TimelineSemaphoreSubmitInfo::default()
            .wait_semaphore_values(&wait_values)
            .signal_semaphore_values(&signal_values);
        let mut info = vk::SubmitInfo::default()
            .command_buffers(&buffers)
            .wait_semaphores(&waits)
            .wait_dst_stage_mask(&stages)
            .signal_semaphores(&signals);
        if timeline {
            info = info.push_next(&mut values);
        }
        let fence = work.commands.fence;
        let ticket = self.pending.track(work)?;
        // SAFETY: The context serializes this queue; frame ownership retains commands, resources and synchronization handles until fence completion.
        unsafe {
            let submit = || self.device.queue_submit(self.queue, &[info], fence);
            #[cfg(test)]
            let result = match self.injected_submit_error.take() {
                Some(error) => Err(error),
                None => submit(),
            };
            #[allow(
                clippy::cfg_not_test,
                reason = "Tests inject queue failures; their normal path calls the same submit closure"
            )]
            #[cfg(not(test))]
            let result = submit();
            if let Err(error) = result {
                if matches!(
                    error,
                    vk::Result::ERROR_OUT_OF_HOST_MEMORY | vk::Result::ERROR_OUT_OF_DEVICE_MEMORY
                ) {
                    // vkQueueSubmit guarantees these rejected calls leave state
                    // untouched; retain earlier prefixes and permit a later retry.
                    drop(self.pending.reject_unsubmitted(&ticket)?);
                } else {
                    self.failed = true;
                }
                return Err(error.into());
            }
        }
        self.pending.confirm(&ticket)?;
        Ok(ticket)
    }
    pub const fn unconfirmed(&self) -> bool {
        self.failed
    }
    #[cfg(test)]
    pub fn submit(&mut self, command: &super::program::Probe) -> Result<Ticket> {
        self.submit_batch(&[command.clone().into()])
    }

    pub fn readback_batch(&mut self, ticket: &Ticket) -> Result<Vec<Vec<u8>>> {
        let fence = self.pending.get(ticket)?.commands.fence;
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        // SAFETY: The pending frame owns the fence and resources throughout the bounded completion wait.
        unsafe {
            if let Err(error) = self.device.wait_for_fences(&[fence], true, 30_000_000_000) {
                self.failed = true;
                return Err(error.into());
            }
        }
        let work = self.pending.take_completed(ticket, ticket.serial())?;
        let result = work.readback();
        // Only a successfully waited fence permits reuse. Never recycle on Drop,
        // rejection, timeout or unknown completion (device-loss quarantine).
        if let Some(staging) = work.upload {
            self.frame_cache.staging.push(staging);
        }
        if let Some(arena) = work.gpu {
            self.frame_cache.retire_storage(arena);
        }
        self.frame_cache.commands.push(work.commands);
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
        self.pending.len()
    }

    pub fn is_complete(&mut self, ticket: &Ticket) -> Result<bool> {
        let fence = self.pending.get(ticket)?.commands.fence;
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        // SAFETY: The pending frame retains this same-device fence throughout the status query.
        match unsafe { self.device.get_fence_status(fence) } {
            Ok(complete) => Ok(complete),
            Err(error) => {
                self.failed = true;
                Err(error.into())
            }
        }
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        // SAFETY: Pending owners are released only after completion; failed waits quarantine the device and resources instead of destroying in-flight handles.
        unsafe {
            if !super::submissions::can_release_after_wait(self.failed, || {
                let fences: Vec<_> = self
                    .pending
                    .values()
                    .map(|work| work.commands.fence)
                    .collect();
                if fences.is_empty() {
                    Ok(())
                } else {
                    self.device.wait_for_fences(&fences, true, 30_000_000_000)
                }
            }) {
                // Unknown completion cannot release frame owners or callback data.
                // This disposable verification process releases quarantined objects
                // at exit; no production device-loss recovery is claimed here.
                self.messages
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(
                        "native Vulkan cleanup could not confirm completion; resources quarantined"
                            .into(),
                    );
                self.pending.quarantine();
                std::mem::forget(std::mem::take(&mut self.compute_pipelines));
                std::mem::forget((
                    self.entry.clone(),
                    std::sync::Arc::clone(&self.messages),
                    self.host_owner.take(),
                ));
                return;
            }
            self.pending.clear_after_completion();
            self.frame_cache = frame_cache::Cache::default();
            self.compute_pipelines.clear();
            if self.host_owner.is_none() {
                self.device.destroy_device(None);
            }
            if self.messenger != vk::DebugUtilsMessengerEXT::null() {
                self.debug
                    .destroy_debug_utils_messenger(self.messenger, None);
            }
            if self.host_owner.is_none() {
                self.instance.destroy_instance(None);
            }
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) mod compute_texture;

#[path = "vulkan/compute_sampler.rs"]
mod compute_sampler;

#[cfg(test)]
impl Vulkan {
    pub(crate) fn import_descriptor(
        &self,
        owner: std::rc::Rc<dyn std::any::Any>,
    ) -> crate::native::interop::vulkan::ContextDescriptor {
        crate::native::interop::vulkan::ContextDescriptor {
            entry: self.entry.clone(),
            instance: self.instance.clone(),
            device: self.device.clone(),
            physical_device: self.physical,
            queue: self.queue,
            queue_family: self.family,
            texture_tables: self.texture_tables,
            timeline_semaphores: self.timeline_semaphores,
            validation: true,
            owner,
        }
    }
}

#[path = "vulkan/synchronization.rs"]
mod synchronization;

#[cfg(test)]
mod memory_cache_tests;

mod frame_cache;

#[cfg(test)]
mod command_cache_tests;
#[cfg(test)]
mod sampler_tests;
