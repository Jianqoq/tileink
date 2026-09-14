//! Vulkan descriptor/image limits must be checked before void recording calls;
//! successful allocation does not establish that a descriptor range is legal.
use super::{Dispatch, Result};

pub(super) fn validate(command: &Dispatch, buffer_bytes: u32, image_width: u32) -> Result<()> {
    if command.source().len() as u64 > buffer_bytes as u64
        || command.destination().len() as u64 > buffer_bytes as u64
        || command
            .texture()
            .is_some_and(|(width, _)| width > image_width)
    {
        return Err("native Vulkan resource exceeds physical device limits".into());
    }
    Ok(())
}

#[test]
fn descriptor_range_and_texture_extent_cannot_exceed_device_limits() {
    let mut command = super::super::program::Probe {
        entry: "copy_words",
        params: super::super::program::Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0, 0, 1, 0],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    };
    assert!(validate(&command.clone().into(), 4, 1).is_ok());
    assert!(validate(&command.clone().into(), 3, 1).is_err());
    command.source.resize(8, 0);
    assert!(validate(&command.clone().into(), 4, 1).is_err());
    command.source.resize(4, 0);
    command.destination.resize(8, 0);
    assert!(validate(&command.clone().into(), 4, 1).is_err());
    command.destination.resize(4, 0);
    command.entry = "sample_words";
    assert!(validate(&command.clone().into(), 4, 0).is_err());
    assert!(validate(&command.clone().into(), 4, 1).is_ok());
}

/// Vulkan only guarantees 128 invocations. Reject a 256-lane renderer kernel
/// before creating its pipeline on devices that cannot execute that workgroup.
pub(super) fn workgroup(group: [u32; 3], dimensions: [u32; 3], invocations: u32) -> Result<()> {
    if group
        .into_iter()
        .zip(dimensions)
        .any(|(size, limit)| size == 0 || size > limit)
        || group
            .into_iter()
            .try_fold(1u64, |total, size| total.checked_mul(size as u64))
            .is_none_or(|total| total > invocations as u64)
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
            BindingKind::TextureWrite => 3,
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
