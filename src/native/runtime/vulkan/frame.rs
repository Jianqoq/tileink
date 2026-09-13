//! Vulkan command/descriptors/buffers owned by one pending queue submission.
use super::{Dispatch, Result};
use ash::vk;

pub struct Frame {
    images: Vec<texture::Image>,
    device: ash::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    pool: vk::CommandPool,
    descriptors: vk::DescriptorPool,
    pub fence: vk::Fence,
    pub command: vk::CommandBuffer,
    buffers: Vec<Buffer>,
    size: usize,
}
impl Frame {
    pub fn record(
        device: &ash::Device,
        memory: vk::PhysicalDeviceMemoryProperties,
        family: u32,
        bindings: vk::DescriptorSetLayout,
        layout: vk::PipelineLayout,
        pipeline: vk::Pipeline,
        case: &Dispatch,
    ) -> Result<Self> {
        unsafe {
            let mut frame = Self {
                device: device.clone(),
                memory,
                pool: vk::CommandPool::null(),
                descriptors: vk::DescriptorPool::null(),
                fence: vk::Fence::null(),
                command: vk::CommandBuffer::null(),
                buffers: Vec::new(),
                images: Vec::new(),
                size: case.destination().len(),
            };
            frame.pool = frame.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                None,
            )?;
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 1,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 2,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1,
                },
            ];
            frame.descriptors = frame.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?;
            frame.fence = frame
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)?;
            let destination =
                frame.buffer(case.destination(), vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let source = frame.buffer(case.source(), vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let params = if let Some(params) = case.params() {
                Some(frame.buffer(
                    bytemuck::bytes_of(params),
                    vk::BufferUsageFlags::UNIFORM_BUFFER,
                )?)
            } else {
                None
            };
            let layouts = [bindings];
            let set = frame.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(frame.descriptors)
                    .set_layouts(&layouts),
            )?[0];
            let infos = [
                [vk::DescriptorBufferInfo {
                    buffer: destination,
                    offset: 0,
                    range: case.destination().len() as u64,
                }],
                [vk::DescriptorBufferInfo {
                    buffer: source,
                    offset: 0,
                    range: case.source().len() as u64,
                }],
                [vk::DescriptorBufferInfo {
                    buffer: params.unwrap_or(vk::Buffer::null()),
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
            frame
                .device
                .update_descriptor_sets(&writes[..if params.is_some() { 3 } else { 2 }], &[]);
            let command = frame.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(frame.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            frame
                .device
                .begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
            if let Some((width, bytes)) = case.texture() {
                let view = frame.record_texture(command, width, bytes)?;
                frame.device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(3)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&[vk::DescriptorImageInfo::default()
                            .image_view(view)
                            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)])],
                    &[],
                );
            }
            frame
                .device
                .cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, pipeline);
            frame.device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                layout,
                0,
                &[set],
                &[],
            );
            if case.workgroups() != 0 {
                frame.device.cmd_dispatch(command, case.workgroups(), 1, 1);
            }
            let barrier = [vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(destination)
                .offset(0)
                .size(vk::WHOLE_SIZE)];
            frame.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &barrier,
                &[],
            );
            frame.device.end_command_buffer(command)?;

            frame.command = command;
            Ok(frame)
        }
    }
    pub fn readback(&self) -> Result<Vec<u8>> {
        unsafe {
            let memory = self.buffers[0].memory;
            let pointer =
                self.device
                    .map_memory(memory, 0, self.size as u64, vk::MemoryMapFlags::empty())?;
            let output = std::slice::from_raw_parts(pointer.cast::<u8>(), self.size).to_vec();
            self.device.unmap_memory(memory);

            Ok(output)
        }
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
    fn clear_buffers(&mut self) {
        unsafe {
            for buffer in self.buffers.drain(..) {
                self.device.destroy_buffer(buffer.buffer, None);
                self.device.free_memory(buffer.memory, None);
            }
        }
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            for image in self.images.drain(..) {
                self.device.destroy_image_view(image.view, None);
                self.device.destroy_image(image.image, None);
                self.device.free_memory(image.memory, None);
            }
            self.clear_buffers();
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_fence(self.fence, None);
        }
    }
}

struct Buffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

#[path = "texture.rs"]
mod texture;
