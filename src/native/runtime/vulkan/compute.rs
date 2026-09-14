//! One command buffer retains GPU intermediates across every pass in a batch.
use super::super::compute::Resource;
use super::compute_texture::Image;
enum GpuResource {
    Buffer(vk::Buffer),
    Sampler(super::compute_sampler::Sampler),
    Image(Image),
}
impl GpuResource {
    fn buffer(&self) -> vk::Buffer {
        match self {
            Self::Buffer(buffer) => *buffer,
            Self::Image(_) | Self::Sampler(_) => unreachable!("validated buffer binding"),
        }
    }
}
use super::super::{Result, compute::ComputeBatch};
use super::{
    compute_memory::{Arena, align},
    compute_pipeline::{Pipeline, descriptor},
};
use crate::native::shaders::BindingKind;
use ash::vk;
use std::collections::BTreeMap;

pub struct Frame {
    device: ash::Device,
    pool: vk::CommandPool,
    descriptors: vk::DescriptorPool,
    pub fence: vk::Fence,
    pub command: vk::CommandBuffer,
    gpu: Option<Arena>,
    resources: Vec<GpuResource>,
    upload: Option<Arena>,
    readback: Option<Arena>,
    outputs: Vec<(usize, usize)>,
    readback_size: usize,
}
impl Frame {
    pub fn record(
        device: &ash::Device,
        memory: &vk::PhysicalDeviceMemoryProperties,
        properties: &vk::PhysicalDeviceProperties,
        family: u32,
        batch: &ComputeBatch,
        pipelines: &BTreeMap<&'static str, Pipeline>,
    ) -> Result<Self> {
        let limits = &properties.limits;
        for pass in batch.passes() {
            if pass
                .grid
                .iter()
                .zip(limits.max_compute_work_group_count)
                .any(|(n, limit)| *n > limit)
            {
                return Err("native Vulkan dispatch exceeds device limit".into());
            }
            for (binding, id) in &pass.bindings {
                if binding.kind == BindingKind::Sampler {
                    continue;
                }
                if let Resource::Texture(texture) = &batch.resources()[id.index()] {
                    if texture
                        .size
                        .iter()
                        .any(|&n| n > limits.max_image_dimension2_d)
                        || texture.layers > limits.max_image_array_layers
                    {
                        return Err("native Vulkan texture exceeds device dimensions".into());
                    }
                    continue;
                }
                let size = if binding.kind == BindingKind::Uniform {
                    binding.size as usize
                } else {
                    batch.size(*id)?
                };
                let limit = if binding.kind == BindingKind::Uniform {
                    limits.max_uniform_buffer_range
                } else {
                    limits.max_storage_buffer_range
                };
                if size > limit as usize {
                    return Err("native Vulkan descriptor range exceeds device limit".into());
                }
            }
        }
        let mut this = Self {
            device: device.clone(),
            pool: vk::CommandPool::null(),
            descriptors: vk::DescriptorPool::null(),
            fence: vk::Fence::null(),
            command: vk::CommandBuffer::null(),
            gpu: None,
            resources: Vec::new(),
            upload: None,
            readback: None,
            outputs: Vec::new(),
            readback_size: 0,
        };
        let mut upload = Vec::new();
        let mut source_offsets = Vec::new();
        let mut grids = Vec::new();
        for buffer in batch.resources() {
            source_offsets.push(upload.len() as u64);
            upload.extend_from_slice(buffer.bytes());
        }
        for pass in batch.passes() {
            let offset = usize::try_from(align(
                upload.len() as u64,
                limits.min_uniform_buffer_offset_alignment.max(4),
            )?)?;
            upload.resize(offset, 0);
            grids.push(offset as u64);
            for word in [pass.grid[0], pass.grid[1], pass.grid[2], 0] {
                upload.extend_from_slice(&word.to_le_bytes());
            }
        }
        for id in batch.outputs() {
            let size = batch.size(*id)?;
            this.outputs.push((this.readback_size, size));
            this.readback_size = this
                .readback_size
                .checked_add(size)
                .ok_or("native readback size overflow")?;
        }
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        this.gpu = Some(Arena::new(
            device,
            memory,
            &batch
                .resources()
                .iter()
                .filter_map(|b| {
                    if let Resource::Buffer(bytes) = b {
                        Some(bytes.len() as u64)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::UNIFORM_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?);
        let mut buffers = this.gpu.as_ref().unwrap().buffers.iter();
        let mut resource_device = None;
        for resource in batch.resources() {
            this.resources.push(match resource {
                Resource::Buffer(_) => GpuResource::Buffer(*buffers.next().unwrap()),
                Resource::Sampler(filter) => {
                    let shared =
                        resource_device.get_or_insert_with(|| std::rc::Rc::new(device.clone()));
                    GpuResource::Sampler(super::compute_sampler::Sampler::new(shared, *filter)?)
                }
                Resource::Texture(texture) => {
                    let shared =
                        resource_device.get_or_insert_with(|| std::rc::Rc::new(device.clone()));
                    GpuResource::Image(Image::new(shared, memory, texture)?)
                }
            });
        }
        if !upload.is_empty() {
            let arena = Arena::new(
                device,
                memory,
                &[upload.len() as u64],
                vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::UNIFORM_BUFFER,
                host,
            )?;
            arena.write(&upload)?;
            this.upload = Some(arena);
        }
        if this.readback_size != 0 {
            this.readback = Some(Arena::new(
                device,
                memory,
                &[this.readback_size as u64],
                vk::BufferUsageFlags::TRANSFER_DST,
                host,
            )?);
        }
        unsafe {
            this.pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                None,
            )?;
            this.fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            this.command = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(this.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            let command = this.command;
            let mut counts = [0u32; 5];
            for pass in batch.passes() {
                for b in pass.shader.bindings {
                    let i = match b.kind {
                        BindingKind::Sampler => 4,
                        BindingKind::Uniform => 0,
                        BindingKind::Read | BindingKind::Write => 1,
                        BindingKind::Texture | BindingKind::TextureArray => 2,
                        BindingKind::TextureWrite => 3,
                    };
                    counts[i] = counts[i]
                        .checked_add(1)
                        .ok_or("native Vulkan descriptor count overflow")?;
                }
            }
            if !batch.passes().is_empty() {
                let sizes: Vec<_> = [
                    vk::DescriptorType::UNIFORM_BUFFER,
                    vk::DescriptorType::STORAGE_BUFFER,
                    vk::DescriptorType::SAMPLED_IMAGE,
                    vk::DescriptorType::STORAGE_IMAGE,
                    vk::DescriptorType::SAMPLER,
                ]
                .into_iter()
                .zip(counts)
                .filter(|(_, n)| *n != 0)
                .map(|(ty, descriptor_count)| vk::DescriptorPoolSize {
                    ty,
                    descriptor_count,
                })
                .collect();
                this.descriptors = device.create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(u32::try_from(batch.passes().len())?)
                        .pool_sizes(&sizes),
                    None,
                )?;
            }
            device.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
            let gpu = &this.resources;
            for (i, buffer) in batch.resources().iter().enumerate() {
                if matches!(buffer, Resource::Sampler(_)) {
                    continue;
                }
                if let GpuResource::Image(image) = &gpu[i] {
                    image.upload(
                        command,
                        this.upload.as_ref().unwrap().buffers[0],
                        source_offsets[i],
                    );
                    continue;
                }
                device.cmd_copy_buffer(
                    command,
                    this.upload.as_ref().unwrap().buffers[0],
                    gpu[i].buffer(),
                    &[vk::BufferCopy {
                        src_offset: source_offsets[i],
                        dst_offset: 0,
                        size: buffer.bytes().len() as u64,
                    }],
                );
            }
            barrier(
                device,
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ
                    | vk::AccessFlags::SHADER_WRITE
                    | vk::AccessFlags::UNIFORM_READ,
            );
            for (index, pass) in batch.passes().iter().enumerate() {
                let pipeline = &pipelines[pass.shader.entry];
                let set = device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(this.descriptors)
                        .set_layouts(&[pipeline.bindings]),
                )?[0];
                for binding in pass.shader.bindings {
                    if binding.kind == BindingKind::Sampler {
                        let id = pass
                            .bindings
                            .iter()
                            .find(|(b, _)| b.slot == binding.slot)
                            .unwrap()
                            .1;
                        let GpuResource::Sampler(sampler) = &gpu[id.index()] else {
                            unreachable!("validated sampler")
                        };
                        device.update_descriptor_sets(
                            &[vk::WriteDescriptorSet::default()
                                .dst_set(set)
                                .dst_binding(binding.slot)
                                .descriptor_type(vk::DescriptorType::SAMPLER)
                                .image_info(&[
                                    vk::DescriptorImageInfo::default().sampler(sampler.handle)
                                ])],
                            &[],
                        );
                        continue;
                    }
                    if matches!(
                        binding.kind,
                        BindingKind::Texture
                            | BindingKind::TextureWrite
                            | BindingKind::TextureArray
                    ) {
                        let id = pass
                            .bindings
                            .iter()
                            .find(|(b, _)| b.slot == binding.slot)
                            .unwrap()
                            .1;
                        let GpuResource::Image(image) = &gpu[id.index()] else {
                            unreachable!("validated image binding")
                        };
                        device.update_descriptor_sets(
                            &[vk::WriteDescriptorSet::default()
                                .dst_set(set)
                                .dst_binding(binding.slot)
                                .descriptor_type(descriptor(binding.kind))
                                .image_info(&[vk::DescriptorImageInfo::default()
                                    .image_view(image.view)
                                    .image_layout(vk::ImageLayout::GENERAL)])],
                            &[],
                        );
                        continue;
                    }
                    let (buffer, offset, range) = if binding.internal {
                        (
                            this.upload.as_ref().unwrap().buffers[0],
                            grids[index],
                            binding.size as u64,
                        )
                    } else {
                        let id = pass
                            .bindings
                            .iter()
                            .find(|(b, _)| b.slot == binding.slot)
                            .unwrap()
                            .1;
                        (
                            gpu[id.index()].buffer(),
                            0,
                            if binding.kind == BindingKind::Uniform {
                                binding.size as u64
                            } else {
                                batch.size(id)? as u64
                            },
                        )
                    };
                    device.update_descriptor_sets(
                        &[vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(binding.slot)
                            .descriptor_type(descriptor(binding.kind))
                            .buffer_info(&[vk::DescriptorBufferInfo {
                                buffer,
                                offset,
                                range,
                            }])],
                        &[],
                    );
                }
                device.cmd_bind_pipeline(
                    command,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.pipeline,
                );
                device.cmd_bind_descriptor_sets(
                    command,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.layout,
                    0,
                    &[set],
                    &[],
                );
                device.cmd_dispatch(command, pass.grid[0], pass.grid[1], pass.grid[2]);
                barrier(
                    device,
                    command,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::AccessFlags::SHADER_WRITE,
                    vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::SHADER_WRITE
                        | vk::AccessFlags::UNIFORM_READ,
                );
            }
            if let Some(readback) = &this.readback {
                barrier(
                    device,
                    command,
                    vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::AccessFlags::TRANSFER_WRITE | vk::AccessFlags::SHADER_WRITE,
                    vk::AccessFlags::TRANSFER_READ,
                );
                for (id, (offset, size)) in batch.outputs().iter().zip(&this.outputs) {
                    if let GpuResource::Image(image) = &gpu[id.index()] {
                        image.readback(command, readback.buffers[0], *offset as u64);
                        continue;
                    }
                    device.cmd_copy_buffer(
                        command,
                        gpu[id.index()].buffer(),
                        readback.buffers[0],
                        &[vk::BufferCopy {
                            src_offset: 0,
                            dst_offset: *offset as u64,
                            size: *size as u64,
                        }],
                    );
                }
                barrier(
                    device,
                    command,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::HOST,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::HOST_READ,
                );
            }
            device.end_command_buffer(command)?;
        }
        Ok(this)
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        let Some(buffer) = &self.readback else {
            return Ok(Vec::new());
        };
        let bytes = buffer.read(self.readback_size)?;
        Ok(self
            .outputs
            .iter()
            .map(|(offset, size)| bytes[*offset..offset + size].to_vec())
            .collect())
    }
}
unsafe fn barrier(
    device: &ash::Device,
    command: vk::CommandBuffer,
    src: vk::PipelineStageFlags,
    dst: vk::PipelineStageFlags,
    read: vk::AccessFlags,
    write: vk::AccessFlags,
) {
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            src,
            dst,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(read)
                .dst_access_mask(write)],
            &[],
            &[],
        );
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_pool(self.descriptors, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_fence(self.fence, None);
        }
    }
}
