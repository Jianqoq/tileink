//! Vulkan descriptor/image limits must be checked before void recording calls;
//! successful allocation does not establish that a descriptor range is legal.
use super::Result;

/// Vulkan only guarantees 128 invocations. Reject a 256-lane renderer kernel
/// before creating its pipeline on devices that cannot execute that workgroup.
pub(super) fn workgroup(group: [u32; 3], dimensions: [u32; 3], invocations: u32) -> Result<()> {
    if group
        .into_iter()
        .zip(dimensions)
        .any(|(size, limit)| size == 0 || size > limit)
        || group
            .into_iter()
            .try_fold(1u64, |total, size| total.checked_mul(u64::from(size)))
            .is_none_or(|total| total > u64::from(invocations))
    {
        return Err("native Vulkan workgroup exceeds physical device limits".into());
    }
    Ok(())
}
#[test]
fn shader_workgroup_must_fit_both_axis_and_invocation_limits() {
    assert!(workgroup([256, 1, 1], [256, 256, 64], 256).is_ok());
    assert!(workgroup([256, 1, 1], [128, 256, 64], 256).is_err());
    assert!(workgroup([256, 1, 1], [256, 256, 64], 128).is_err());
    assert!(workgroup([0, 1, 1], [256, 256, 64], 256).is_err());
    assert!(workgroup([u32::MAX; 3], [u32::MAX; 3], u32::MAX).is_err());
}

/// Count every array element before creating Vulkan layouts. One array binding
/// can exceed the device limit even when the number of bindings is small.
pub(super) fn descriptors(
    bindings: &[crate::native::shaders::Binding],
    limits: &ash::vk::PhysicalDeviceLimits,
) -> Result<()> {
    use crate::native::shaders::BindingKind;
    let mut counts = [0u64; 5];
    for binding in bindings {
        let index = match binding.kind {
            BindingKind::Uniform => 0,
            BindingKind::Read | BindingKind::Write => 1,
            BindingKind::Texture | BindingKind::TextureArray | BindingKind::TextureTable => 2,
            BindingKind::TextureWrite | BindingKind::TextureReadWrite => 3,
            BindingKind::Sampler => 4,
        };
        counts[index] += u64::from(binding.count);
    }
    let maxima = [
        limits
            .max_per_stage_descriptor_uniform_buffers
            .min(limits.max_descriptor_set_uniform_buffers),
        limits
            .max_per_stage_descriptor_storage_buffers
            .min(limits.max_descriptor_set_storage_buffers),
        limits
            .max_per_stage_descriptor_sampled_images
            .min(limits.max_descriptor_set_sampled_images),
        limits
            .max_per_stage_descriptor_storage_images
            .min(limits.max_descriptor_set_storage_images),
        limits
            .max_per_stage_descriptor_samplers
            .min(limits.max_descriptor_set_samplers),
    ];
    if counts
        .into_iter()
        .zip(maxima)
        .any(|(count, maximum)| count > u64::from(maximum))
        || counts[..4].iter().sum::<u64>() > u64::from(limits.max_per_stage_resources)
    {
        return Err("native Vulkan descriptors exceed physical device limits".into());
    }
    Ok(())
}

#[test]
fn texture_table_elements_count_toward_stage_and_set_limits() {
    use crate::native::shaders::{Binding, BindingKind};
    let bindings = [Binding {
        slot: 0,
        kind: BindingKind::TextureTable,
        size: 4,
        count: 8,
        internal: false,
    }];
    let mut limits = ash::vk::PhysicalDeviceLimits {
        max_per_stage_descriptor_sampled_images: 8,
        max_descriptor_set_sampled_images: 8,
        max_per_stage_resources: 8,
        ..Default::default()
    };
    assert!(descriptors(&bindings, &limits).is_ok());
    limits.max_per_stage_descriptor_sampled_images = 7;
    assert!(descriptors(&bindings, &limits).is_err());
    limits.max_per_stage_descriptor_sampled_images = 8;
    limits.max_descriptor_set_sampled_images = 7;
    assert!(descriptors(&bindings, &limits).is_err());
    limits.max_descriptor_set_sampled_images = 8;
    limits.max_per_stage_resources = 7;
    assert!(descriptors(&bindings, &limits).is_err());
}

/// Validate the resources actually bound by each pass before native recording.
/// Verification programs and renderer kernels must obey the same device limits.
pub(super) fn resources(
    batch: &super::super::compute::ComputeBatch,
    limits: &ash::vk::PhysicalDeviceLimits,
) -> Result<()> {
    use super::super::compute::Resource;
    use crate::native::shaders::BindingKind;
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
    Ok(())
}

#[test]
fn compute_descriptors_and_image_extents_obey_device_limits() {
    use super::super::program::{Params, Probe, record_batch};
    let mut command = Probe {
        entry: "copy_words",
        params: Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0, 0, 1, 0],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    };
    let mut limits = ash::vk::PhysicalDeviceLimits {
        max_compute_work_group_count: [65535; 3],
        max_storage_buffer_range: 4,
        max_uniform_buffer_range: 32,
        max_image_dimension2_d: 1,
        max_image_array_layers: 1,
        ..Default::default()
    };
    let check = |command: &Probe, limits: &ash::vk::PhysicalDeviceLimits| {
        resources(&record_batch(&[command.clone().into()]).unwrap(), limits)
    };
    assert!(check(&command, &limits).is_ok());
    limits.max_storage_buffer_range = 3;
    assert!(check(&command, &limits).is_err());
    limits.max_storage_buffer_range = 4;
    command.source.resize(8, 0);
    assert!(check(&command, &limits).is_err());
    command.source.resize(4, 0);
    command.destination.resize(8, 0);
    assert!(check(&command, &limits).is_err());
    command.destination.resize(4, 0);
    limits.max_uniform_buffer_range = 31;
    assert!(check(&command, &limits).is_err());
    limits.max_uniform_buffer_range = 32;
    command.entry = "sample_words";
    assert!(check(&command, &limits).is_ok());
    limits.max_image_dimension2_d = 0;
    assert!(check(&command, &limits).is_err());
    limits.max_image_dimension2_d = 1;
    limits.max_compute_work_group_count = [0, 1, 1];
    assert!(check(&command, &limits).is_err());
}
