use tileink::native_interop::vulkan::{
    ImageState, SemaphorePoint, SemaphoreWait, TargetSynchronization,
};
use tileink::{NativeRenderTarget, NativeTargetState, NativeTargetSubmission, NativeTargetUse};
#[path = "vulkan_capabilities.rs"]
mod capabilities;
use super::app::Result;
use ash::{Entry, vk};
use std::rc::Rc;
use tileink::{
    NativeContext, NativeSubmission, NativeTexture,
    native_interop::vulkan::{ContextDescriptor, TextureDescriptor},
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};
struct Platform {
    entry: Entry,
    instance: ash::Instance,
    surface_api: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
}
impl Drop for Platform {
    fn drop(&mut self) {
        unsafe {
            self.surface_api.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}
struct Device {
    platform: Rc<Platform>,
    device: ash::Device,
    physical: vk::PhysicalDevice,
    queue: vk::Queue,
    swapchain_api: ash::khr::swapchain::Device,
}
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
        }
    }
}
struct Swapchain {
    device: Rc<Device>,
    handle: vk::SwapchainKHR,
    finished: Vec<vk::Semaphore>,
    acquired: Vec<vk::Semaphore>,
}
impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            for semaphore in self.finished.iter().chain(&self.acquired) {
                self.device.device.destroy_semaphore(*semaphore, None);
            }
            self.device
                .swapchain_api
                .destroy_swapchain(self.handle, None);
        }
    }
}
pub struct Host {
    images: Vec<NativeTexture>,
    pending: Vec<Option<NativeSubmission>>,
    initialized: Vec<bool>,
    context: NativeContext,
    swapchain: Option<Rc<Swapchain>>,
    device: Rc<Device>,
    size: [u32; 2],
    slot: usize,
    image: usize,
}
impl Host {
    pub fn new(window: &Window) -> Result<Self> {
        unsafe {
            let entry = Entry::load()?;
            let extensions = [
                ash::khr::surface::NAME.as_ptr(),
                ash::khr::win32_surface::NAME.as_ptr(),
                ash::ext::debug_utils::NAME.as_ptr(),
            ];
            let layers = [c"VK_LAYER_KHRONOS_validation".as_ptr()];
            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(
                        &vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2),
                    )
                    .enabled_extension_names(&extensions)
                    .enabled_layer_names(&layers),
                None,
            )?;
            let surface_api = ash::khr::surface::Instance::new(&entry, &instance);
            let mut platform = Platform {
                entry,
                instance,
                surface_api,
                surface: vk::SurfaceKHR::null(),
            };
            let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
                return Err("expected Win32 window".into());
            };
            platform.surface =
                ash::khr::win32_surface::Instance::new(&platform.entry, &platform.instance)
                    .create_win32_surface(
                        &vk::Win32SurfaceCreateInfoKHR::default()
                            .hwnd(handle.hwnd.get())
                            .hinstance(handle.hinstance.ok_or("missing HINSTANCE")?.get()),
                        None,
                    )?;
            let platform = Rc::new(platform);
            let mut selected = None;
            for physical in platform.instance.enumerate_physical_devices()? {
                for (index, family) in platform
                    .instance
                    .get_physical_device_queue_family_properties(physical)
                    .iter()
                    .enumerate()
                {
                    if family
                        .queue_flags
                        .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                        && platform.surface_api.get_physical_device_surface_support(
                            physical,
                            index as u32,
                            platform.surface,
                        )?
                    {
                        selected = Some((physical, index as u32));
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
            let (physical, family) = selected.ok_or("no Vulkan compute/present queue")?;
            let priorities = [1.0];
            let queues = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&priorities)];
            let mut indexing = vk::PhysicalDeviceDescriptorIndexingFeatures::default()
                .shader_sampled_image_array_non_uniform_indexing(true);
            let device = platform.instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&[ash::khr::swapchain::NAME.as_ptr()])
                    .push_next(&mut indexing),
                None,
            )?;
            let queue = device.get_device_queue(family, 0);
            let swapchain_api = ash::khr::swapchain::Device::new(&platform.instance, &device);
            let owner = Rc::new(Device {
                platform,
                device,
                physical,
                queue,
                swapchain_api,
            });
            let context = NativeContext::from_vulkan(ContextDescriptor {
                entry: owner.platform.entry.clone(),
                instance: owner.platform.instance.clone(),
                physical_device: physical,
                device: owner.device.clone(),
                queue,
                queue_family: family,
                texture_tables: true,
                timeline_semaphores: false,
                validation: true,
                owner: owner.clone(),
            })?;
            let this = Self {
                images: Vec::new(),
                pending: vec![None, None],
                initialized: Vec::new(),
                context,
                swapchain: None,
                device: owner,
                size: [0; 2],
                slot: 0,
                image: 0,
            };
            Ok(this)
        }
    }
    fn resize(&mut self, size: [u32; 2]) -> Result {
        unsafe {
            // Resize is a host policy boundary; retire render and presentation work.
            self.device.device.device_wait_idle()?;
            for pending in &mut self.pending {
                if let Some(submission) = pending.take() {
                    submission.wait()?;
                }
            }
            self.images.clear();
            self.swapchain = None;
            let owner = &self.device;
            let capabilities = owner
                .platform
                .surface_api
                .get_physical_device_surface_capabilities(owner.physical, owner.platform.surface)?;
            let formats = owner
                .platform
                .surface_api
                .get_physical_device_surface_formats(owner.physical, owner.platform.surface)?;
            let format = formats
                .iter()
                .find(|f| {
                    f.format == vk::Format::R8G8B8A8_UNORM
                        && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
                })
                .ok_or("this example requires an RGBA8 UNORM swapchain")?;
            let usage = vk::ImageUsageFlags::STORAGE
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST;
            if !capabilities.supported_usage_flags.contains(usage) {
                return Err("surface lacks the required native target usages".into());
            }
            let (extent, count) = capabilities::select(&capabilities, size)?;
            let alpha = [
                vk::CompositeAlphaFlagsKHR::OPAQUE,
                vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
                vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
                vk::CompositeAlphaFlagsKHR::INHERIT,
            ]
            .into_iter()
            .find(|flag| capabilities.supported_composite_alpha.contains(*flag))
            .ok_or("no composite alpha mode")?;
            let handle = owner.swapchain_api.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(owner.platform.surface)
                    .min_image_count(count)
                    .image_format(format.format)
                    .image_color_space(format.color_space)
                    .image_extent(extent)
                    .image_array_layers(1)
                    .image_usage(usage)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(capabilities.current_transform)
                    .composite_alpha(alpha)
                    .present_mode(vk::PresentModeKHR::FIFO)
                    .clipped(true),
                None,
            )?;
            let mut swapchain = Swapchain {
                device: owner.clone(),
                handle,
                finished: Vec::new(),
                acquired: Vec::new(),
            };
            let raw_images = owner.swapchain_api.get_swapchain_images(handle)?;
            for _ in &raw_images {
                swapchain.finished.push(
                    owner
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                );
            }
            for _ in 0..2 {
                swapchain.acquired.push(
                    owner
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                );
            }
            self.initialized = vec![false; raw_images.len()];
            let swapchain = Rc::new(swapchain);
            for image in raw_images {
                self.images
                    .push(self.context.import_vulkan_texture(TextureDescriptor {
                        image,
                        size,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                        initialized: false,
                        owner: swapchain.clone(),
                    })?);
            }
            self.swapchain = Some(swapchain);
            self.size = size;
            Ok(())
        }
    }
}
impl super::app::Host for Host {
    fn context(&self) -> &NativeContext {
        &self.context
    }
    fn acquire(&mut self, size: [u32; 2]) -> Result<NativeTexture> {
        if self.size != size {
            self.resize(size)?;
        }
        if let Some(submission) = self.pending[self.slot].take() {
            submission.wait()?;
        }
        unsafe {
            let (image, _) = match self.device.swapchain_api.acquire_next_image(
                self.swapchain.as_ref().unwrap().handle,
                u64::MAX,
                self.swapchain.as_ref().unwrap().acquired[self.slot],
                vk::Fence::null(),
            ) {
                Ok(image) => image,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.resize(size)?;
                    self.device.swapchain_api.acquire_next_image(
                        self.swapchain.as_ref().unwrap().handle,
                        u64::MAX,
                        self.swapchain.as_ref().unwrap().acquired[self.slot],
                        vk::Fence::null(),
                    )?
                }
                Err(error) => return Err(error.into()),
            };
            self.image = image as usize;
        }
        Ok(self.images[self.image].clone())
    }
    fn target_use<'a>(&self, target: NativeRenderTarget<'a>) -> Result<NativeTargetUse<'a>> {
        let swapchain = self.swapchain.as_ref().unwrap();
        let incoming = ImageState {
            layout: if self.initialized[self.image] {
                vk::ImageLayout::PRESENT_SRC_KHR
            } else {
                vk::ImageLayout::UNDEFINED
            },
            stages: vk::PipelineStageFlags::TOP_OF_PIPE,
            access: vk::AccessFlags::empty(),
            queue_family: vk::QUEUE_FAMILY_IGNORED,
        };
        let outgoing = ImageState {
            layout: vk::ImageLayout::PRESENT_SRC_KHR,
            stages: vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            access: vk::AccessFlags::empty(),
            queue_family: vk::QUEUE_FAMILY_IGNORED,
        };
        Ok(unsafe {
            self.context.vulkan_target_use(
                target,
                TargetSynchronization {
                    incoming,
                    outgoing,
                    waits: vec![SemaphoreWait {
                        semaphore: SemaphorePoint::Binary(swapchain.acquired[self.slot]),
                        stages: vk::PipelineStageFlags::ALL_COMMANDS,
                    }],
                    signals: vec![SemaphorePoint::Binary(swapchain.finished[self.image])],
                    owner: swapchain.clone(),
                },
            )?
        })
    }
    fn present(&mut self, submission: NativeTargetSubmission) -> Result {
        if !matches!(submission.outgoing, NativeTargetState::Vulkan(state) if state.layout == vk::ImageLayout::PRESENT_SRC_KHR)
        {
            return Err("unexpected Vulkan output layout".into());
        }
        self.pending[self.slot] = Some(submission.submission);
        self.initialized[self.image] = true;
        let swapchain = self.swapchain.as_ref().unwrap();
        unsafe {
            let finished = [swapchain.finished[self.image]];
            match self.device.swapchain_api.queue_present(
                self.device.queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&finished)
                    .swapchains(&[swapchain.handle])
                    .image_indices(&[self.image as u32]),
            ) {
                Ok(_) => {}
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.size = [0; 2],
                Err(error) => return Err(error.into()),
            }
        }
        self.slot = (self.slot + 1) % self.pending.len();
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            if self.device.device.device_wait_idle().is_err() {
                // Do not destroy WSI semaphores or imported images with unknown completion.
                std::mem::forget((
                    self.device.clone(),
                    self.swapchain.clone(),
                    self.images.clone(),
                    self.context.clone(),
                ));
            }
        }
    }
}
