use std::num::NonZeroU32;

use crate::shared::{gpu_layout::fine as fine_layout, image_resource::MAX_IMAGE_RESOURCE_TEXTURES};

use super::canvas::WgpuImageResourceBindings;

pub(crate) use super::shader_variants::patch_image_resource_shader_source;

pub(crate) fn large_texture_table_len(device: &::wgpu::Device) -> u32 {
    let features = device.features();
    if !features.contains(::wgpu::Features::TEXTURE_BINDING_ARRAY)
        || !features.contains(
            ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
        )
    {
        return 0;
    }
    let limits = device.limits();
    let sampled_limit = limits
        .max_sampled_textures_per_shader_stage
        .saturating_sub(4);
    let binding_array_limit = limits.max_binding_array_elements_per_shader_stage;
    sampled_limit
        .min(binding_array_limit)
        .min(MAX_IMAGE_RESOURCE_TEXTURES as u32)
}

pub(crate) fn create_image_resource_bind_group_layout(
    device: &::wgpu::Device,
    large_texture_table_len: u32,
) -> ::wgpu::BindGroupLayout {
    let entries = image_resource_layout_entries(large_texture_table_len);
    device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some("tileink wgpu image resource bind group layout"),
        entries: &entries,
    })
}

pub(crate) fn image_resource_layout_entries(
    large_texture_table_len: u32,
) -> Vec<::wgpu::BindGroupLayoutEntry> {
    let mut entries = vec![
        sampled_filterable_array_texture_layout_entry(fine_layout::IMAGE_RESOURCE_ATLAS_BINDING),
        filtering_sampler_layout_entry(fine_layout::IMAGE_RESOURCE_SAMPLER_BINDING),
    ];
    if large_texture_table_len > 0 {
        entries.push(sampled_filterable_texture_table_layout_entry(
            fine_layout::IMAGE_RESOURCE_TEXTURES_BINDING,
            large_texture_table_len,
        ));
    }
    entries
}

pub(crate) fn create_image_resource_bind_group(
    device: &::wgpu::Device,
    layout: &::wgpu::BindGroupLayout,
    bindings: &WgpuImageResourceBindings<'_>,
    large_texture_table_len: u32,
) -> ::wgpu::BindGroup {
    let mut entries = vec![
        texture_binding(fine_layout::IMAGE_RESOURCE_ATLAS_BINDING, bindings.atlas),
        sampler_binding(
            fine_layout::IMAGE_RESOURCE_SAMPLER_BINDING,
            bindings.sampler,
        ),
    ];
    let texture_table;
    if large_texture_table_len > 0 {
        texture_table = image_resource_texture_table(bindings, large_texture_table_len as usize);
        entries.push(::wgpu::BindGroupEntry {
            binding: fine_layout::IMAGE_RESOURCE_TEXTURES_BINDING,
            resource: ::wgpu::BindingResource::TextureViewArray(&texture_table),
        });
    }
    device.create_bind_group(&::wgpu::BindGroupDescriptor {
        label: Some("tileink wgpu image resource bind group"),
        layout,
        entries: &entries,
    })
}

fn image_resource_texture_table<'a>(
    bindings: &'a WgpuImageResourceBindings<'a>,
    len: usize,
) -> Vec<&'a ::wgpu::TextureView> {
    let mut textures = Vec::with_capacity(len);
    for index in 0..len {
        textures.push(
            bindings
                .texture_views
                .get(index)
                .unwrap_or(bindings.dummy_texture),
        );
    }
    textures
}

fn sampled_filterable_array_texture_layout_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Texture {
            sample_type: ::wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: ::wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}

fn sampled_filterable_texture_table_layout_entry(
    binding: u32,
    count: u32,
) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Texture {
            sample_type: ::wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: ::wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: NonZeroU32::new(count),
    }
}

fn filtering_sampler_layout_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Sampler(::wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn texture_binding(binding: u32, view: &::wgpu::TextureView) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::TextureView(view),
    }
}

fn sampler_binding(binding: u32, sampler: &::wgpu::Sampler) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::Sampler(sampler),
    }
}
