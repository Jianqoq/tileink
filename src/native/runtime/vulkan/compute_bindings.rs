//! Vulkan descriptor writes are kept separate from command recording and transfers.
use super::*;
use crate::native::runtime::compute::Pass;

impl Frame {
    pub(super) fn write_bindings(
        &self,
        batch: &ComputeBatch,
        pass: &Pass,
        set: vk::DescriptorSet,
        grid_offset: u64,
    ) -> Result<()> {
        let device = &self.device;
        let gpu = &self.resources;
        unsafe {
            for binding in pass.shader.bindings {
                let id = if binding.internal {
                    None
                } else {
                    Some(
                        pass.bindings
                            .iter()
                            .find(|(b, _)| b.slot == binding.slot)
                            .unwrap()
                            .1,
                    )
                };
                if binding.kind == BindingKind::TextureTable {
                    let id = id.unwrap();
                    let Resource::TextureTable(images) = &batch.resources()[id.index()] else {
                        unreachable!("validated table")
                    };
                    let views: Vec<_> = images
                        .iter()
                        .map(|id| {
                            let GpuResource::Image(image) = &gpu[id.index()] else {
                                unreachable!("validated table image")
                            };
                            vk::DescriptorImageInfo::default()
                                .image_view(image.view)
                                .image_layout(vk::ImageLayout::GENERAL)
                        })
                        .collect();
                    device.update_descriptor_sets(
                        &[vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(binding.slot)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&views)],
                        &[],
                    );
                    continue;
                }
                if binding.kind == BindingKind::Sampler {
                    let id = id.unwrap();
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
                    BindingKind::Texture | BindingKind::TextureWrite | BindingKind::TextureArray
                ) {
                    let id = id.unwrap();
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
                        self.upload.as_ref().unwrap().buffers[0],
                        grid_offset,
                        binding.size as u64,
                    )
                } else {
                    let id = id.unwrap();
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
        }
        Ok(())
    }
}
