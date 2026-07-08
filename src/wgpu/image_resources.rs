use std::num::NonZeroU32;

use crate::shared::{gpu_layout::fine as fine_layout, image_resource::MAX_IMAGE_RESOURCE_TEXTURES};

use super::canvas::WgpuImageResourceBindings;

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
    device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some("tileink wgpu image resource bind group layout"),
        entries: &entries,
    })
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

pub(crate) fn patch_image_resource_shader_source(
    source: &'static str,
    large_texture_table_enabled: bool,
) -> String {
    let binding = if large_texture_table_enabled {
        "@group(1) @binding(2) var image_resource_textures: binding_array<texture_2d<f32>>;"
    } else {
        ""
    };
    let functions = if large_texture_table_enabled {
        LARGE_TEXTURE_TABLE_FUNCTIONS
    } else {
        LARGE_TEXTURE_TABLE_DISABLED_FUNCTIONS
    };
    let patched = source
        .replace("// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_BINDING", binding)
        .replace(
            "// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_FUNCTIONS",
            functions,
        );
    if large_texture_table_enabled {
        format!("enable wgpu_binding_array;\n{patched}")
    } else {
        patched
    }
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

const LARGE_TEXTURE_TABLE_DISABLED_FUNCTIONS: &str = r#"
fn sample_resource_pattern_texture(
    tx: f32,
    ty: f32,
    texture_index: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    return 0u;
}
"#;

const LARGE_TEXTURE_TABLE_FUNCTIONS: &str = r#"
fn sample_resource_pattern_texture(
    tx: f32,
    ty: f32,
    texture_index: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (sampling == GPU_PATTERN_BILINEAR && extend == 0u) {
        let dims = vec2<f32>(textureDimensions(image_resource_textures[texture_index]));
        let local = vec2<f32>(
            clamp(tx, 0.0, f32(width)),
            clamp(ty, 0.0, f32(height)),
        );
        let uv = local / dims;
        color = unorm_to_rgba8(textureSampleLevel(image_resource_textures[texture_index], image_resource_sampler, uv, 0.0));
    } else if (sampling == GPU_PATTERN_BILINEAR) {
        let sx = tx - 0.5;
        let sy = ty - 0.5;
        let x0f = floor(sx);
        let y0f = floor(sy);
        let fx = sx - x0f;
        let fy = sy - y0f;
        let x0 = i32(x0f);
        let y0 = i32(y0f);
        let tl = texture_pattern_pixel(texture_index, width, height, extend, x0, y0);
        let tr = texture_pattern_pixel(texture_index, width, height, extend, x0 + 1, y0);
        let bl = texture_pattern_pixel(texture_index, width, height, extend, x0, y0 + 1);
        let br = texture_pattern_pixel(texture_index, width, height, extend, x0 + 1, y0 + 1);
        color = lerp_premul_u8(lerp_premul_u8(tl, tr, fx), lerp_premul_u8(bl, br, fx), fy);
    } else {
        color = texture_pattern_pixel(texture_index, width, height, extend, i32(floor(tx)), i32(floor(ty)));
    }
    return scale_premul_u8(color, opacity);
}

fn texture_pattern_pixel(texture_index: u32, width: u32, height: u32, extend: u32, x: i32, y: i32) -> u32 {
    let local_x = extend_coord_i32(x, width, extend);
    let local_y = extend_coord_i32(y, height, extend);
    return unorm_to_rgba8(textureLoad(image_resource_textures[texture_index], vec2<i32>(i32(local_x), i32(local_y)), 0));
}
"#;
