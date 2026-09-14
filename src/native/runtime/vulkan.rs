mod compute;
mod compute_memory;
mod compute_pipeline;
mod frame;
mod limits;
mod pipeline;
mod validation;
mod work;
use super::submissions::{Pending, Ticket};
use frame::Frame;
// Real native Vulkan compute execution for the M2 ABI probes.
use super::{Result, program::Dispatch};
use ash::{Entry, vk};
use std::collections::BTreeMap;

pub struct Vulkan {
    _entry: Entry,
    debug: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    instance: ash::Instance,
    device: ash::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    bindings: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipelines: BTreeMap<&'static str, vk::Pipeline>,
    pending: Pending<work::Work>,
    properties: vk::PhysicalDeviceProperties,
    compute_pipelines: BTreeMap<&'static str, compute_pipeline::Pipeline>,
    family: u32,
    max_storage_buffer_bytes: u32,
    max_image_width: u32,
    failed: bool,
    #[cfg(test)]
    injected_submit_error: Option<vk::Result>,
}

impl Vulkan {
    pub fn new(identity: &str) -> Result<Self> {
        // All native handles remain owned by this context until GPU completion.
        unsafe {
            let entry = Entry::load()?;
            let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let layers = [c"VK_LAYER_KHRONOS_validation".as_ptr()];
            let extensions = [ash::ext::debug_utils::NAME.as_ptr()];
            let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(validation::callback))
                .user_data(std::sync::Arc::as_ptr(&messages).cast_mut().cast());
            let info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&info)
                    .enabled_layer_names(&layers)
                    .enabled_extension_names(&extensions)
                    .push_next(&mut debug_info),
                None,
            )?;
            let selection = (|| -> Result<_> {
                for physical in instance.enumerate_physical_devices()? {
                    let mut id = vk::PhysicalDeviceIDProperties::default();
                    let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
                    instance.get_physical_device_properties2(physical, &mut props);
                    if id.device_luid_valid == 0 {
                        continue;
                    }
                    let actual: String =
                        id.device_luid.iter().map(|b| format!("{b:02x}")).collect();
                    if actual != identity {
                        continue;
                    }
                    let family = instance
                        .get_physical_device_queue_family_properties(physical)
                        .iter()
                        .position(|p| p.queue_flags.contains(vk::QueueFlags::COMPUTE))
                        .ok_or("no compute queue")? as u32;
                    return Ok((physical, family));
                }
                Err("requested native Vulkan physical GPU unavailable".into())
            })();
            let (physical, family) = match selection {
                Ok(v) => v,
                Err(e) => {
                    instance.destroy_instance(None);
                    return Err(e);
                }
            };
            let priorities = [1.0];
            let queues = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&priorities)];
            let device = match instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default().queue_create_infos(&queues),
                None,
            ) {
                Ok(v) => v,
                Err(e) => {
                    instance.destroy_instance(None);
                    return Err(e.into());
                }
            };
            let memory = instance.get_physical_device_memory_properties(physical);
            let properties = instance.get_physical_device_properties(physical);
            let limits = properties.limits;
            let queue = device.get_device_queue(family, 0);
            let debug = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let mut this = Self {
                _entry: entry,
                debug,
                messenger: vk::DebugUtilsMessengerEXT::null(),
                messages,
                instance,
                device,
                memory,
                queue,
                bindings: vk::DescriptorSetLayout::null(),
                layout: vk::PipelineLayout::null(),
                pipelines: BTreeMap::new(),
                pending: Pending::new(),
                properties,
                compute_pipelines: BTreeMap::new(),
                family,
                max_storage_buffer_bytes: limits.max_storage_buffer_range,
                max_image_width: limits.max_image_dimension2_d,
                failed: false,
                #[cfg(test)]
                injected_submit_error: None,
            };
            this.messenger = this.debug.create_debug_utils_messenger(&debug_info, None)?;
            pipeline::create(&mut this, physical)?;
            Ok(this)
        }
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
        for pass in batch.passes() {
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
        )?;
        self.submit_work(work::Work::Compute(Box::new(frame)))
    }
    fn submit_work(&mut self, work: work::Work) -> Result<Ticket> {
        let buffers = work.commands();
        let fence = work.fence()?;
        let ticket = self.pending.track(work)?;
        unsafe {
            let submit = || {
                self.device.queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&buffers)],
                    fence,
                )
            };
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
        self.pending
            .take_completed(ticket, ticket.serial())?
            .readback()
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
                std::mem::forget((self._entry.clone(), self.messages.clone()));
                return;
            }
            self.pending.clear_after_completion();
            self.compute_pipelines.clear();
            for pipeline in self.pipelines.values() {
                self.device.destroy_pipeline(*pipeline, None);
            }
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device
                .destroy_descriptor_set_layout(self.bindings, None);
            self.device.destroy_device(None);
            self.debug
                .destroy_debug_utils_messenger(self.messenger, None);
            self.instance.destroy_instance(None);
        }
    }
}

#[cfg(test)]
mod tests;

mod compute_texture;
