//! Real native Vulkan compute execution for the M2 ABI probes.
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
    pool: vk::CommandPool,
    descriptors: vk::DescriptorPool,
    bindings: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipelines: BTreeMap<&'static str, vk::Pipeline>,
    buffers: Vec<Buffer>,
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
                pool: vk::CommandPool::null(),
                descriptors: vk::DescriptorPool::null(),
                bindings: vk::DescriptorSetLayout::null(),
                layout: vk::PipelineLayout::null(),
                pipelines: BTreeMap::new(),
                buffers: Vec::new(),
            };
            this.messenger = this.debug.create_debug_utils_messenger(&debug_info, None)?;
            this.pool = this.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                None,
            )?;
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
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 2,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1,
                },
            ];
            this.descriptors = this.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
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

    fn buffer(&mut self, bytes: &[u8], usage: vk::BufferUsageFlags) -> Result<vk::Buffer> {
        unsafe {
            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(bytes.len() as u64)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            self.buffers.push(Buffer {
                buffer,
                memory: vk::DeviceMemory::null(),
            });
            let requirements = self.device.get_buffer_memory_requirements(buffer);
            let index = (0..self.memory.memory_type_count)
                .find(|i| {
                    requirements.memory_type_bits & (1 << i) != 0
                        && self.memory.memory_types[*i as usize]
                            .property_flags
                            .contains(
                                vk::MemoryPropertyFlags::HOST_VISIBLE
                                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                            )
                })
                .ok_or("host coherent memory unavailable")?;
            let memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )?;
            self.buffers.last_mut().unwrap().memory = memory;
            self.device.bind_buffer_memory(buffer, memory, 0)?;
            let pointer = self.device.map_memory(
                memory,
                0,
                bytes.len() as u64,
                vk::MemoryMapFlags::empty(),
            )?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            self.device.unmap_memory(memory);
            Ok(buffer)
        }
    }

    pub fn execute(&mut self, case: &Case) -> Result<Vec<u8>> {
        unsafe {
            self.device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())?;
            self.device
                .reset_descriptor_pool(self.descriptors, vk::DescriptorPoolResetFlags::empty())?;
            self.clear_buffers();
            let destination =
                self.buffer(&case.destination, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let source = self.buffer(&case.source, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let params = self.buffer(
                bytemuck::bytes_of(&case.params),
                vk::BufferUsageFlags::UNIFORM_BUFFER,
            )?;
            let layouts = [self.bindings];
            let set = self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.descriptors)
                    .set_layouts(&layouts),
            )?[0];
            let infos = [
                [vk::DescriptorBufferInfo {
                    buffer: destination,
                    offset: 0,
                    range: case.destination.len() as u64,
                }],
                [vk::DescriptorBufferInfo {
                    buffer: source,
                    offset: 0,
                    range: case.source.len() as u64,
                }],
                [vk::DescriptorBufferInfo {
                    buffer: params,
                    offset: 0,
                    range: 32,
                }],
            ];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&infos[0]),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&infos[1]),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&infos[2]),
            ];
            self.device.update_descriptor_sets(&writes, &[]);
            let command = self.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            self.device
                .begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
            self.device.cmd_bind_pipeline(
                command,
                vk::PipelineBindPoint::COMPUTE,
                self.pipelines[case.entry],
            );
            self.device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                self.layout,
                0,
                &[set],
                &[],
            );
            self.device
                .cmd_dispatch(command, case.params.count.div_ceil(64).max(1), 1, 1);
            let barrier = [vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(destination)
                .offset(0)
                .size(vk::WHOLE_SIZE)];
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &barrier,
                &[],
            );
            self.device.end_command_buffer(command)?;
            let commands = [command];
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&commands)],
                vk::Fence::null(),
            )?;
            self.device.queue_wait_idle(self.queue)?;
            let memory = self.buffers[0].memory;
            let pointer = self.device.map_memory(
                memory,
                0,
                case.destination.len() as u64,
                vk::MemoryMapFlags::empty(),
            )?;
            let output =
                std::slice::from_raw_parts(pointer.cast::<u8>(), case.destination.len()).to_vec();
            self.device.unmap_memory(memory);
            self.device.free_command_buffers(self.pool, &commands);
            Ok(output)
        }
    }

    fn clear_buffers(&mut self) {
        unsafe {
            for buffer in self.buffers.drain(..) {
                self.device.destroy_buffer(buffer.buffer, None);
                self.device.free_memory(buffer.memory, None);
            }
        }
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.clear_buffers();
            for pipeline in self.pipelines.values() {
                self.device.destroy_pipeline(*pipeline, None);
            }
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device
                .destroy_descriptor_set_layout(self.bindings, None);
            self.device.destroy_command_pool(self.pool, None);
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
