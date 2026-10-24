//! Vulkan descriptor writes are kept separate from command recording and transfers.
use super::*;
use crate::native::runtime::compute::Pass;

impl Frame {
    #[allow(
        clippy::expect_used,
        reason = "Batch reflection validates every external binding; internal bindings have a planned upload arena"
    )]
    pub(super) fn write_bindings(
        &self,
        batch: &ComputeBatch,
        pass: &Pass,
        set: vk::DescriptorSet,
        grid_offset: u64,
    ) -> Result<()> {
        let device = &self.device;
        let gpu = &self.resources;
        // SAFETY: Validated reflection and resource indices determine same-device descriptor writes; backing arrays and resource owners remain live.
        unsafe {
            for binding in pass.artifact().bindings {
                let id = if binding.internal {
                    None
                } else {
                    Some(
                        pass.bindings
                            .iter()
                            .find(|(b, _)| b.slot == binding.slot)
                            .expect("Validated bindings and upload planning provide this reflected resource")
                            .1,
                    )
                };
                if binding.kind == BindingKind::TextureTable {
                    let id = id.expect(
                        "Validated bindings and upload planning provide this reflected resource",
                    );
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
                    let id = id.expect(
                        "Validated bindings and upload planning provide this reflected resource",
                    );
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
                        | BindingKind::TextureReadWrite
                        | BindingKind::TextureArray
                ) {
                    let id = id.expect(
                        "Validated bindings and upload planning provide this reflected resource",
                    );
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
                        self.upload.as_ref().expect("Validated bindings and upload planning provide this reflected resource").arena.buffers[0],
                        grid_offset,
                        u64::from(binding.size),
                    )
                } else {
                    let id = id.expect(
                        "Validated bindings and upload planning provide this reflected resource",
                    );
                    (
                        gpu[id.index()].buffer(),
                        self.uniform_offsets[id.index()].unwrap_or(0),
                        if binding.kind == BindingKind::Uniform {
                            u64::from(binding.size)
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
