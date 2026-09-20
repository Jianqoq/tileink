mod compute;
pub(super) mod compute_memory;
mod compute_pipeline;
mod context;
mod frame;
mod limits;
mod pipeline;
mod staging;
mod upload;
mod validation;
mod work;
use super::submissions::{Pending, Ticket};
use frame::Frame;
// Real native Vulkan compute execution for the M2 ABI probes.
use super::{Result, program::Dispatch};
use ash::{Entry, vk};
use std::collections::BTreeMap;

pub struct Vulkan {
    host_owner: Option<std::rc::Rc<dyn std::any::Any>>,
    _entry: Entry,
    debug: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    instance: ash::Instance,
    #[cfg(test)]
    physical: vk::PhysicalDevice,
    device: ash::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    bindings: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipelines: BTreeMap<&'static str, vk::Pipeline>,
    pending: Pending<work::Work>,
    staging: Option<staging::Staging>,
    properties: vk::PhysicalDeviceProperties,
    compute_pipelines: BTreeMap<&'static str, compute_pipeline::Pipeline>,
    family: u32,
    max_storage_buffer_bytes: u32,
    max_image_width: u32,
    failed: bool,
    texture_tables: bool,
    timeline_semaphores: bool,
    families: Vec<vk::QueueFamilyProperties>,
    #[cfg(test)]
    injected_submit_error: Option<vk::Result>,
    #[cfg(test)]
    inject_probe_init_failure: bool,
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
    ) -> Result<super::texture::Allocation> {
        let descriptor = super::compute::Texture {
            size,
            layers,
            array,
            bytes: Vec::new(),
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
        self.messages.clone()
    }

    pub fn submit_batch(&mut self, commands: &[Dispatch]) -> Result<Ticket> {
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        super::program::validate_batch(commands)?;
        for command in commands {
            limits::validate(command, self.max_storage_buffer_bytes, self.max_image_width)?;
        }
        if !commands.is_empty() {
            pipeline::ensure(self)?;
        }
        let frames = commands
            .iter()
            .map(|command| {
                Frame::record(
                    &self.device,
                    self.memory,
                    self.family,
                    self.bindings,
                    self.layout,
                    self.pipelines[command.entry()],
                    command,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        self.submit_work(work::Work::Probes(frames))
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
                pass.shader.entry,
            )?;
        }
        let frame = compute::Frame::record(
            &self.device,
            &self.memory,
            &self.properties,
            self.family,
            batch,
            &self.compute_pipelines,
            &mut self.staging,
        )?;
        self.submit_work(work::Work::Compute(Box::new(frame)))
    }
    fn submit_work(&mut self, work: work::Work) -> Result<Ticket> {
        let buffers = work.commands();
        let synchronization = match &work {
            work::Work::Compute(frame) => frame.synchronization.clone(),
            _ => None,
        };
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
        let fence = work.fence()?;
        let ticket = self.pending.track(work)?;
        unsafe {
            let submit = || self.device.queue_submit(self.queue, &[info], fence);
            #[cfg(test)]
            let result = match self.injected_submit_error.take() {
                Some(error) => Err(error),
                None => submit(),
            };
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
    pub fn unconfirmed(&self) -> bool {
        self.failed
    }
    #[cfg(test)]
    pub fn submit(&mut self, command: &super::program::Probe) -> Result<Ticket> {
        self.submit_batch(&[command.clone().into()])
    }

    pub fn readback_batch(&mut self, ticket: &Ticket) -> Result<Vec<Vec<u8>>> {
        let fence = self.pending.get(ticket)?.fence()?;
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        unsafe {
            if let Err(error) = self.device.wait_for_fences(&[fence], true, 30_000_000_000) {
                self.failed = true;
                return Err(error.into());
            }
        }
        let mut work = self.pending.take_completed(ticket, ticket.serial())?;
        let result = work.readback();
        // Only a successfully waited fence permits reuse. Never recycle on Drop,
        // rejection, timeout or unknown completion (device-loss quarantine).
        if let work::Work::Compute(frame) = &mut work
            && let Some(staging) = frame.upload.take()
        {
            self.staging = Some(staging);
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
        self.pending.len()
    }

    pub fn is_complete(&mut self, ticket: &Ticket) -> Result<bool> {
        let fence = self.pending.get(ticket)?.fence()?;
        if self.failed {
            return Err("Vulkan context failed".into());
        }
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
        unsafe {
            if !super::submissions::can_release_after_wait(self.failed, || {
                let fences: Vec<_> = self
                    .pending
                    .values()
                    .filter_map(|work| work.fence().ok())
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
                    .unwrap_or_else(|error| error.into_inner())
                    .push(
                        "native Vulkan cleanup could not confirm completion; resources quarantined"
                            .into(),
                    );
                self.pending.quarantine();
                std::mem::forget(std::mem::take(&mut self.compute_pipelines));
                std::mem::forget((
                    self._entry.clone(),
                    self.messages.clone(),
                    self.host_owner.take(),
                ));
                return;
            }
            self.pending.clear_after_completion();
            self.staging = None;
            self.compute_pipelines.clear();
            for pipeline in self.pipelines.values() {
                self.device.destroy_pipeline(*pipeline, None);
            }
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device
                .destroy_descriptor_set_layout(self.bindings, None);
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
            entry: self._entry.clone(),
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
