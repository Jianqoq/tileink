//! One command buffer retains GPU intermediates across every pass in a batch.
#[path = "compute_bindings.rs"]
mod bindings;
use super::super::compute::Resource;
use super::compute_texture::Image;
enum GpuResource {
    TextureTable,
    Buffer(vk::Buffer),
    Sampler(std::rc::Rc<super::compute_sampler::Sampler>),
    Image(std::rc::Rc<Image>),
}
impl GpuResource {
    fn buffer(&self) -> vk::Buffer {
        match self {
            Self::Buffer(buffer) => *buffer,
            Self::Image(_) | Self::Sampler(_) | Self::TextureTable => {
                unreachable!("validated buffer binding")
            }
        }
    }
}
use super::super::{Result, compute::ComputeBatch};
use super::{
    compute_memory::Arena,
    compute_pipeline::{Pipeline, descriptor},
};
use crate::native::shaders::BindingKind;
use ash::vk;
use std::collections::BTreeMap;

pub struct Frame {
    pub synchronization: Option<crate::native::interop::vulkan::TargetSynchronization>,
    device: ash::Device,
    pub(super) commands: super::frame_cache::Commands,
    pub(super) gpu: Option<Arena>,
    persistent_buffers: Vec<std::rc::Rc<Arena>>,
    resources: Vec<GpuResource>,
    pub(super) upload: Option<super::staging::Staging>,
    readback: Option<Arena>,
    outputs: Vec<(usize, usize)>,
    readback_size: usize,
    uniform_offsets: Vec<Option<u64>>,
}
impl Frame {
    #[cfg(test)]
    pub(super) fn sampler_identities(&self) -> Vec<usize> {
        self.resources
            .iter()
            .filter_map(|resource| {
                if let GpuResource::Sampler(sampler) = resource {
                    Some(std::rc::Rc::as_ptr(sampler) as usize)
                } else {
                    None
                }
            })
            .collect()
    }
    #[cfg(test)]
    pub(super) fn command_handles(&self) -> (vk::CommandPool, vk::DescriptorPool, vk::Fence) {
        self.commands.handles()
    }
    #[cfg(test)]
    pub(super) fn storage_buffers(&self) -> &[vk::Buffer] {
        &self.gpu.as_ref().unwrap().buffers
    }
    pub fn record(
        device: &ash::Device,
        memory: &vk::PhysicalDeviceMemoryProperties,
        properties: &vk::PhysicalDeviceProperties,
        family: u32,
        batch: &ComputeBatch,
        pipelines: &BTreeMap<&'static str, Pipeline>,
        frame_cache: &mut super::frame_cache::Cache,
    ) -> Result<Self> {
        let limits = &properties.limits;
        // Every owned image is allocated, including images reachable only through a table.
        for resource in batch.resources() {
            if let Resource::Texture(texture) = resource
                && (texture
                    .size
                    .iter()
                    .any(|&n| n > limits.max_image_dimension2_d)
                    || texture.layers > limits.max_image_array_layers)
            {
                return Err("native Vulkan texture exceeds device dimensions".into());
            }
        }
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
                if matches!(
                    binding.kind,
                    BindingKind::Sampler | BindingKind::TextureTable
                ) {
                    continue;
                }
                if matches!(&batch.resources()[id.index()], Resource::Texture(_)) {
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
        let synchronization = match &batch.synchronization {
            Some((id, crate::native::interop::Synchronization::Vulkan(sync))) => Some((*id, sync)),
            None => None,
            #[cfg(feature = "dx12")]
            _ => return Err("DX12 synchronization requires a DX12 context".into()),
        };
        let mut this = Self {
            synchronization: synchronization.map(|(_, sync)| sync.clone()),
            device: device.clone(),
            commands: frame_cache.acquire_commands(device, family, batch.passes())?,
            gpu: None,
            persistent_buffers: Vec::new(),
            resources: Vec::new(),
            upload: None,
            readback: None,
            outputs: Vec::new(),
            readback_size: 0,
            uniform_offsets: Vec::new(),
        };
        let uniforms = crate::native::runtime::compute::uniforms::Uniforms::new(
            batch,
            usize::try_from(limits.min_uniform_buffer_offset_alignment.max(4))?,
        )?;
        this.uniform_offsets = uniforms.offsets;
        let mut upload = super::upload::Upload::new();
        upload.push(&uniforms.bytes, 1)?;
        let mut source_offsets = Vec::new();
        let mut grids = Vec::new();
        for (index, buffer) in batch.resources().iter().enumerate() {
            if let Some(offset) = this.uniform_offsets[index] {
                source_offsets.push(offset);
                continue;
            }
            source_offsets.push(upload.push(buffer.bytes(), 1)? as u64);
        }
        let grid_bytes: Vec<[u8; 16]> = batch
            .passes()
            .iter()
            .map(|pass| {
                bytemuck::cast([
                    pass.grid[0].to_le(),
                    pass.grid[1].to_le(),
                    pass.grid[2].to_le(),
                    0u32,
                ])
            })
            .collect();
        for bytes in &grid_bytes {
            grids.push(upload.push(
                bytes,
                usize::try_from(limits.min_uniform_buffer_offset_alignment.max(4))?,
            )? as u64);
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
        if upload.len() != 0 {
            this.upload = Some(super::staging::Staging::prepare(
                device,
                memory,
                &upload,
                &mut frame_cache.staging,
            )?);
        }
        this.gpu = Some(
            frame_cache.acquire_storage(
                device,
                memory,
                &batch
                    .resources()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, b)| {
                        if this.uniform_offsets[index].is_some() {
                            return None;
                        }
                        if let Resource::Buffer(bytes) = b {
                            Some(bytes.len() as u64)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
            )?,
        );
        let mut buffers = this.gpu.as_ref().unwrap().buffers.iter();
        let mut resource_device = None;
        for (index, resource) in batch.resources().iter().enumerate() {
            this.resources.push(match resource {
                Resource::TextureTable(_) => GpuResource::TextureTable,
                Resource::PersistentBuffer(upload) => {
                    let crate::native::runtime::buffer::Allocation::Vulkan(allocation) =
                        &upload.buffer.state.allocation;
                    this.persistent_buffers.push(allocation.clone());
                    GpuResource::Buffer(allocation.buffers[0])
                }
                Resource::Buffer(_) => {
                    GpuResource::Buffer(if this.uniform_offsets[index].is_some() {
                        this.upload.as_ref().unwrap().arena.buffers[0]
                    } else {
                        *buffers.next().unwrap()
                    })
                }
                Resource::Sampler(filter) => {
                    GpuResource::Sampler(frame_cache.samplers.get(device, *filter)?)
                }
                Resource::Texture(texture) => {
                    if let Some(texture) = &texture.persistent {
                        let crate::native::runtime::texture::Allocation::Vulkan(image) =
                            &texture.state.allocation;
                        this.resources.push(GpuResource::Image(image.clone()));
                        continue;
                    }
                    let shared =
                        resource_device.get_or_insert_with(|| std::rc::Rc::new(device.clone()));
                    GpuResource::Image(std::rc::Rc::new(Image::new(shared, memory, texture)?))
                }
            });
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
            let command = this.commands.command;
            // A retired slot is reset and recorded again before its next submit;
            // the driver need not preserve this recording for repeated execution.
            device.begin_command_buffer(
                command,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let gpu = &this.resources;
            // Queue order alone does not make previous shader writes/reads visible
            // to range transfers in the next batch. Cover both write and reuse hazards.
            barrier(
                device,
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::MEMORY_READ,
                vk::AccessFlags::TRANSFER_WRITE
                    | vk::AccessFlags::TRANSFER_READ
                    | vk::AccessFlags::SHADER_READ
                    | vk::AccessFlags::SHADER_WRITE,
            );

            for (i, buffer) in batch.resources().iter().enumerate() {
                if this.uniform_offsets[i].is_some()
                    || matches!(buffer, Resource::Sampler(_) | Resource::TextureTable(_))
                {
                    continue;
                }
                if let Resource::PersistentBuffer(input) = buffer {
                    if !input.copies.is_empty() {
                        let copies: Vec<_> = input
                            .copies
                            .iter()
                            .map(|&[source, destination, size]| vk::BufferCopy {
                                src_offset: source_offsets[i] + source,
                                dst_offset: destination,
                                size,
                            })
                            .collect();
                        device.cmd_copy_buffer(
                            command,
                            this.upload.as_ref().unwrap().arena.buffers[0],
                            gpu[i].buffer(),
                            &copies,
                        );
                    }
                    continue;
                }
                if let GpuResource::Image(image) = &gpu[i] {
                    if let Resource::Texture(texture) = buffer
                        && texture.persistent.is_some()
                    {
                        if let Some((_, sync)) = synchronization.filter(|(id, _)| id.index() == i) {
                            image.acquire(command, sync.incoming, family);
                        } else {
                            if image.current_family.get() != vk::QUEUE_FAMILY_IGNORED
                                && image.current_family.get() != family
                            {
                                return Err(
                                    "external Vulkan ownership requires an explicit target use"
                                        .into(),
                                );
                            }
                            image.transition(
                                command,
                                image.current_layout.get(),
                                vk::ImageLayout::GENERAL,
                            );
                        }
                        continue;
                    }
                    image.upload(
                        command,
                        this.upload.as_ref().unwrap().arena.buffers[0],
                        source_offsets[i],
                    );
                    continue;
                }
                device.cmd_copy_buffer(
                    command,
                    this.upload.as_ref().unwrap().arena.buffers[0],
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
            for operation in batch.commands() {
                let index = match operation {
                    crate::native::runtime::compute::Command::Dispatch(index) => *index,
                    crate::native::runtime::compute::Command::CopyTexture(copy) => {
                        let GpuResource::Image(source) = &gpu[copy.source.index()] else {
                            unreachable!("validated copy image")
                        };
                        let GpuResource::Image(destination) = &gpu[copy.destination.index()] else {
                            unreachable!("validated copy image")
                        };
                        source.copy_to(command, destination, copy);
                        continue;
                    }
                };
                let pass = &batch.passes()[index];
                if batch.skip_initialization(pass) {
                    continue;
                }
                let pipeline = &pipelines[pass.shader.entry];
                let set = device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(this.commands.descriptors)
                        .set_layouts(&[pipeline.bindings]),
                )?[0];
                this.write_bindings(batch, pass, set, grids[index])?;
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
                        if matches!(&batch.resources()[id.index()], Resource::Texture(texture) if texture.persistent.is_some())
                        {
                            image.transition(
                                command,
                                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                vk::ImageLayout::GENERAL,
                            );
                        }
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
            // Restore the host's declared layout before its next queue operation.
            for (index, resource) in batch.resources().iter().enumerate() {
                if matches!(resource, Resource::Texture(texture) if texture.persistent.is_some())
                    && let GpuResource::Image(image) = &gpu[index]
                {
                    if let Some((_, sync)) = synchronization.filter(|(id, _)| id.index() == index) {
                        image.release(command, sync.outgoing, family);
                    } else if image.final_layout != vk::ImageLayout::GENERAL {
                        image.transition(command, vk::ImageLayout::GENERAL, image.final_layout);
                    }
                }
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
