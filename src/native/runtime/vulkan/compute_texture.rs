use super::super::{Result, compute::Texture};
use ash::vk;

pub(super) struct Image {
    device: std::rc::Rc<ash::Device>,
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    extent: vk::Extent3D,
}
impl Image {
    pub fn new(
        device: &std::rc::Rc<ash::Device>,
        memory: &vk::PhysicalDeviceMemoryProperties,
        texture: &Texture,
    ) -> Result<Self> {
        let mut this = Self {
            device: device.clone(),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            extent: vk::Extent3D {
                width: texture.size[0],
                height: texture.size[1],
                depth: 1,
            },
        };
        unsafe {
            this.image = device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .extent(this.extent)
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::SAMPLED
                            | vk::ImageUsageFlags::STORAGE
                            | vk::ImageUsageFlags::TRANSFER_SRC
                            | vk::ImageUsageFlags::TRANSFER_DST,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let requirements = device.get_image_memory_requirements(this.image);
            let index = (0..memory.memory_type_count)
                .find(|i| {
                    requirements.memory_type_bits & (1 << i) != 0
                        && memory.memory_types[*i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or("native Vulkan image memory unavailable")?;
            this.memory = device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )?;
            device.bind_image_memory(this.image, this.memory, 0)?;
            this.view = device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(this.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(Self::range()),
                None,
            )?;
        }
        Ok(this)
    }
    fn range() -> vk::ImageSubresourceRange {
        vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1)
    }
    fn region(&self, offset: u64) -> vk::BufferImageCopy {
        vk::BufferImageCopy::default()
            .buffer_offset(offset)
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(self.extent)
    }
    fn transition(
        &self,
        command: vk::CommandBuffer,
        before: vk::ImageLayout,
        after: vk::ImageLayout,
    ) {
        let access = |layout| match layout {
            vk::ImageLayout::UNDEFINED => (
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::AccessFlags::empty(),
            ),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_WRITE,
            ),
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL => (
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_READ,
            ),
            vk::ImageLayout::GENERAL => (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
            ),
            _ => unreachable!("compute image layout"),
        };
        let (src_stage, src_access) = access(before);
        let (dst_stage, dst_access) = access(after);
        unsafe {
            self.device.cmd_pipeline_barrier(
                command,
                src_stage,
                dst_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(self.image)
                    .subresource_range(Self::range())
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .old_layout(before)
                    .new_layout(after)
                    .src_access_mask(src_access)
                    .dst_access_mask(dst_access)],
            );
        }
    }
    pub fn upload(&self, command: vk::CommandBuffer, buffer: vk::Buffer, offset: u64) {
        self.transition(
            command,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        unsafe {
            self.device.cmd_copy_buffer_to_image(
                command,
                buffer,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[self.region(offset)],
            );
        }
        // Keep sampled/storage descriptors in GENERAL across compute passes.
        self.transition(
            command,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::GENERAL,
        );
    }
    pub fn readback(&self, command: vk::CommandBuffer, buffer: vk::Buffer, offset: u64) {
        self.transition(
            command,
            vk::ImageLayout::GENERAL,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        );
        unsafe {
            self.device.cmd_copy_image_to_buffer(
                command,
                self.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[self.region(offset)],
            );
        }
    }
}
impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
