#[path = "vulkan_frame.rs"]
mod frame;
use super::submissions::{Pending, Ticket};
use frame::Frame;
// Real native Vulkan compute execution for the M2 ABI probes.
use super::{Result, cases::Case};
use ash::{Entry, vk};
use std::{collections::BTreeMap, ffi::CString};

struct Buffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

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
    pending: Pending<Frame>,
    family: u32,
    failed: bool,
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
                .pfn_user_callback(Some(validation_callback))
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
                family,
                failed: false,
            };
            this.messenger = this.debug.create_debug_utils_messenger(&debug_info, None)?;
            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
            ];
            this.bindings = this.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layouts = [this.bindings];
            this.layout = this.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
                None,
            )?;
            for artifact in tileink::NATIVE_SHADER_ARTIFACTS
                .iter()
                .filter(|a| a.format == "spirv")
            {
                let words: Vec<u32> = artifact
                    .bytes
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                let shader = this.device.create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&words),
                    None,
                )?;
                let name = CString::new(artifact.entry)?;
                let stage = vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::COMPUTE)
                    .module(shader)
                    .name(&name);
                let properties = this.instance.get_physical_device_properties(physical);
                let identity = serde_json::to_vec(
                    &serde_json::json!({"api":"vulkan","version":properties.api_version,"vendor":properties.vendor_id,"device":properties.device_id,"driver":properties.driver_version,"uuid":properties.pipeline_cache_uuid}),
                )?;
                let pipeline = std::cell::Cell::new(vk::Pipeline::null());
                let build =
                    |data: &[u8]| -> std::result::Result<(vk::Pipeline, Vec<u8>), vk::Result> {
                        let cache = this.device.create_pipeline_cache(
                            &vk::PipelineCacheCreateInfo::default().initial_data(data),
                            None,
                        )?;
                        let result = this.device.create_compute_pipelines(
                            cache,
                            &[vk::ComputePipelineCreateInfo::default()
                                .stage(stage)
                                .layout(this.layout)],
                            None,
                        );
                        let handle = match result {
                            Ok(p) => p[0],
                            Err((partial, error)) => {
                                for p in partial {
                                    this.device.destroy_pipeline(p, None);
                                }
                                this.device.destroy_pipeline_cache(cache, None);
                                return Err(error);
                            }
                        };
                        let bytes = this.device.get_pipeline_cache_data(cache);
                        this.device.destroy_pipeline_cache(cache, None);
                        match bytes {
                            Ok(bytes) => Ok((handle, bytes)),
                            Err(error) => {
                                this.device.destroy_pipeline(handle, None);
                                Err(error)
                            }
                        }
                    };
                let cached = super::pipeline_cache::load_or_create(
                    &identity,
                    artifact.cache_key,
                    |data| {
                        if !super::pipeline_cache::vulkan_header_matches(
                            data,
                            properties.vendor_id,
                            properties.device_id,
                            &properties.pipeline_cache_uuid,
                        ) {
                            return Ok(false);
                        }
                        match build(data) {
                            Ok((handle, _)) => {
                                pipeline.set(handle);
                                Ok(true)
                            }
                            Err(_) => Ok(false),
                        }
                    },
                    || {
                        let (handle, bytes) = build(&[]).map_err(std::io::Error::other)?;
                        pipeline.set(handle);
                        Ok(bytes)
                    },
                );
                this.device.destroy_shader_module(shader, None);
                match cached {
                    Ok(hit) => {
                        eprintln!(
                            "native Vulkan pipeline {}: {}",
                            artifact.entry,
                            if hit { "cache hit" } else { "compiled" }
                        );
                        this.pipelines.insert(artifact.entry, pipeline.get());
                    }
                    Err(error) => {
                        this.device.destroy_pipeline(pipeline.get(), None);
                        return Err(error.into());
                    }
                }
            }
            Ok(this)
        }
    }

    pub fn validation_messages(&self) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        self.messages.clone()
    }

    pub fn submit(&mut self, case: &Case) -> Result<Ticket> {
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        let frame = Frame::record(
            &self.device,
            self.memory,
            self.family,
            self.bindings,
            self.layout,
            self.pipelines[case.entry],
            case,
        )?;
        let ticket = self.pending.track(frame)?;
        let frame = self.pending.get(&ticket)?;
        let commands = [frame.command];
        unsafe {
            if let Err(error) = self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&commands)],
                frame.fence,
            ) {
                self.failed = true;
                return Err(error.into());
            }
        }
        self.pending.confirm(&ticket)?;
        Ok(ticket)
    }
    pub fn readback(&mut self, ticket: &Ticket) -> Result<Vec<u8>> {
        let frame = self.pending.get(ticket)?;
        if self.failed {
            return Err("Vulkan context failed".into());
        }
        unsafe {
            if let Err(error) = self
                .device
                .wait_for_fences(&[frame.fence], true, 30_000_000_000)
            {
                self.failed = true;
                return Err(error.into());
            }
        }
        self.pending
            .take_completed(ticket, ticket.serial())?
            .readback()
    }
    pub fn execute(&mut self, case: &Case) -> Result<Vec<u8>> {
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
                let fences: Vec<_> = self.pending.values().map(|frame| frame.fence).collect();
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
                std::mem::forget((self._entry.clone(), self.messages.clone()));
                return;
            }
            self.pending.clear_after_completion();
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

unsafe extern "system" fn validation_callback(
    _severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    // Vulkan invokes this only while the context owns the Arc backing user.
    unsafe {
        if !data.is_null() && !user.is_null() && !(*data).p_message.is_null() {
            let messages = &*user.cast::<std::sync::Mutex<Vec<String>>>();
            let text = std::ffi::CStr::from_ptr((*data).p_message)
                .to_string_lossy()
                .into_owned();
            messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text);
        }
    }
    vk::FALSE
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn failed_teardown_is_reported() -> Result<()> {
    if super::isolation::run("vulkan::failed_teardown_is_reported")? {
        return Ok(());
    }
    let mut context = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let messages = context.validation_messages();
    context.failed = true;
    drop(context);
    assert!(
        messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("cleanup could not confirm completion"))
    );
    Ok(())
}
