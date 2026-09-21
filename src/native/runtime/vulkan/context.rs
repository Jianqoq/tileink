//! Device selection, creation and immutable rendering limits.
use super::*;
use crate::native::runtime::renderer::recording::Limits;

impl Vulkan {
    #[cfg(test)]
    pub fn new(identity: &str) -> Result<Self> {
        Self::with_options(&crate::native::NativeContextOptions {
            physical_adapter: Some(identity.to_owned()),
            validation: true,
        })
    }

    pub fn with_options(options: &crate::native::NativeContextOptions) -> Result<Self> {
        // All native handles remain owned by this context until GPU completion.
        unsafe {
            let entry = Entry::load()?;
            let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let layers = if options.validation {
                vec![c"VK_LAYER_KHRONOS_validation".as_ptr()]
            } else {
                Vec::new()
            };
            let extensions = if options.validation {
                vec![ash::ext::debug_utils::NAME.as_ptr()]
            } else {
                Vec::new()
            };
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
            let mut instance_info = vk::InstanceCreateInfo::default()
                .application_info(&info)
                .enabled_layer_names(&layers)
                .enabled_extension_names(&extensions);
            if options.validation {
                instance_info = instance_info.push_next(&mut debug_info);
            }
            let instance = entry.create_instance(&instance_info, None)?;
            let selection = (|| -> Result<_> {
                for physical in instance.enumerate_physical_devices()? {
                    let mut id = vk::PhysicalDeviceIDProperties::default();
                    let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
                    instance.get_physical_device_properties2(physical, &mut props);
                    if options.physical_adapter.is_some() && id.device_luid_valid == 0 {
                        continue;
                    }
                    let actual: String =
                        id.device_luid.iter().map(|b| format!("{b:02x}")).collect();
                    if options
                        .physical_adapter
                        .as_ref()
                        .is_some_and(|identity| identity != &actual)
                    {
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
            let mut indexing = vk::PhysicalDeviceDescriptorIndexingFeatures::default();
            let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut indexing);
            instance.get_physical_device_features2(physical, &mut features);
            let extensions = match instance.enumerate_device_extension_properties(physical) {
                Ok(extensions) => extensions,
                Err(error) => {
                    instance.destroy_instance(None);
                    return Err(error.into());
                }
            };
            let texture_tables = indexing.shader_sampled_image_array_non_uniform_indexing != 0
                && extensions.iter().any(|ext| {
                    std::ffi::CStr::from_ptr(ext.extension_name.as_ptr())
                        == ash::ext::descriptor_indexing::NAME
                });
            let table_extensions = if texture_tables {
                vec![ash::ext::descriptor_indexing::NAME.as_ptr()]
            } else {
                Vec::new()
            };
            let mut indexing = vk::PhysicalDeviceDescriptorIndexingFeatures::default()
                .shader_sampled_image_array_non_uniform_indexing(texture_tables);
            let mut device_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queues)
                .enabled_extension_names(&table_extensions);
            if texture_tables {
                device_info = device_info.push_next(&mut indexing);
            }
            let device = match instance.create_device(physical, &device_info, None) {
                Ok(v) => v,
                Err(e) => {
                    instance.destroy_instance(None);
                    return Err(e.into());
                }
            };
            let queue = device.get_device_queue(family, 0);
            Self::from_parts(ContextParts {
                entry,
                instance,
                device,
                physical,
                queue,
                family,
                texture_tables,
                timeline_semaphores: false,
                validation: options.validation,
                messages,
                host_owner: None,
            })
        }
    }

    pub fn from_imported(
        descriptor: crate::native::interop::vulkan::ContextDescriptor,
    ) -> Result<Self> {
        let crate::native::interop::vulkan::ContextDescriptor {
            entry,
            instance,
            physical_device: physical,
            device,
            queue,
            queue_family: family,
            texture_tables,
            timeline_semaphores,
            validation,
            owner,
        } = descriptor;
        unsafe {
            let families = instance.get_physical_device_queue_family_properties(physical);
            if families
                .get(family as usize)
                .is_none_or(|family| !family.queue_flags.contains(vk::QueueFlags::COMPUTE))
                || queue == vk::Queue::null()
            {
                return Err("imported Vulkan queue must support compute".into());
            }
            if instance
                .get_physical_device_properties(physical)
                .api_version
                < vk::API_VERSION_1_1
            {
                return Err("native Vulkan requires API 1.1".into());
            }
        }
        Self::from_parts(ContextParts {
            entry,
            instance,
            physical,
            device,
            queue,
            family,
            texture_tables,
            timeline_semaphores,
            validation,
            messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            host_owner: Some(owner),
        })
    }

    fn from_parts(parts: ContextParts) -> Result<Self> {
        let ContextParts {
            entry,
            instance,
            device,
            physical,
            queue,
            family,
            texture_tables,
            timeline_semaphores,
            validation,
            messages,
            host_owner,
        } = parts;
        unsafe {
            let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
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
            let memory = instance.get_physical_device_memory_properties(physical);
            let properties = instance.get_physical_device_properties(physical);
            let limits = properties.limits;
            let families = instance.get_physical_device_queue_family_properties(physical);
            let debug = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let mut this = Self {
                host_owner,
                _entry: entry,
                debug,
                messenger: vk::DebugUtilsMessengerEXT::null(),
                messages,
                instance,
                #[cfg(test)]
                physical,
                device,
                memory,
                queue,
                bindings: vk::DescriptorSetLayout::null(),
                layout: vk::PipelineLayout::null(),
                pipelines: BTreeMap::new(),
                pending: Pending::new(),
                frame_cache: Default::default(),
                properties,
                compute_pipelines: BTreeMap::new(),
                family,
                max_storage_buffer_bytes: limits.max_storage_buffer_range,
                max_image_width: limits.max_image_dimension2_d,
                failed: false,
                texture_tables,
                timeline_semaphores,
                families,
                #[cfg(test)]
                injected_submit_error: None,
                #[cfg(test)]
                inject_probe_init_failure: false,
            };
            if validation {
                this.messenger = this.debug.create_debug_utils_messenger(&debug_info, None)?;
            }
            Ok(this)
        }
    }

    pub fn limits(&self) -> Limits {
        let limits = self.properties.limits;
        Limits {
            image_dimension: limits.max_image_dimension2_d,
            atlas_pages: limits.max_image_array_layers,
            texture_table_len: if self.texture_tables {
                crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY
            } else {
                0
            },
            dispatch_dimension: limits.max_compute_work_group_count[0]
                .min(limits.max_compute_work_group_count[1]),
        }
    }
}

struct ContextParts {
    entry: Entry,
    instance: ash::Instance,
    device: ash::Device,
    physical: vk::PhysicalDevice,
    queue: vk::Queue,
    family: u32,
    texture_tables: bool,
    timeline_semaphores: bool,
    validation: bool,
    messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    host_owner: Option<std::rc::Rc<dyn std::any::Any>>,
}
