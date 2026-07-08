use crate::text::AtlasSignature;

use super::buffer::WgpuBuffer;

mod bindings;
mod upload;
mod work_buffers;

pub(crate) use bindings::{
    WgpuCoarseBindings, WgpuCumsumBindings, WgpuFilterBindings, WgpuImageResourceBindings,
    WgpuScanBindings, WgpuTileFineBindings,
};
pub(crate) use upload::WgpuSceneUploadStaging;
pub(crate) use work_buffers::{WgpuCoarseBuffers, WgpuScanBuffers};
pub(crate) struct WgpuSceneBuffers {
    lines: WgpuBuffer,
    path_records: WgpuBuffer,
    draw_records: WgpuBuffer,
    sdf_blob: WgpuBuffer,
    sdf_shadow_blob: WgpuBuffer,
    fine_paint_blob: WgpuBuffer,
    scan_chunk_path_ids: WgpuBuffer,
    scan_chunk_backdrop_offsets: WgpuBuffer,
    scan_chunk_segment_starts: WgpuBuffer,
    scan_chunk_lens: WgpuBuffer,
    scan_chunk_range_starts: WgpuBuffer,
    scan_chunk_range_ends: WgpuBuffer,
    cumsum_chunk_backdrop_offsets: WgpuBuffer,
    cumsum_chunk_lens: WgpuBuffer,
    cumsum_row_chunk_starts: WgpuBuffer,
    cumsum_row_chunk_ends: WgpuBuffer,
    plan_layer_stack: WgpuBuffer,
    brush_blob: WgpuBuffer,
    image_resource_atlas: ::wgpu::Texture,
    image_resource_atlas_view: ::wgpu::TextureView,
    // Standalone GPU textures for large image resources that bypass atlas pages.
    image_resource_textures: Vec<::wgpu::Texture>,
    // Texture views bound through the large-image texture table.
    image_resource_texture_views: Vec<::wgpu::TextureView>,
    // 1x1 fallback texture used to pad unused texture-table slots.
    image_resource_dummy_texture: ::wgpu::Texture,
    // View for the dummy texture, shared by fallback and table padding bindings.
    image_resource_dummy_texture_view: ::wgpu::TextureView,
    image_resource_sampler: ::wgpu::Sampler,
    image_resource_atlas_size: (u32, u32, u32),
    text_runs: WgpuBuffer,
    coarse_text_blob: WgpuBuffer,
    fine_text_blob: WgpuBuffer,
    glyph_atlas_signature: AtlasSignature,
    fine_paint_sdf_shadow_base: u32,
    fine_paint_brush_base: u32,
    fine_text_image_base: u32,
    fine_text_image_data_base: u32,
}

impl WgpuSceneBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        let image_resource_atlas = create_image_resource_atlas_texture(device, 1, 1, 1);
        let image_resource_atlas_view = create_image_resource_atlas_view(&image_resource_atlas);
        let image_resource_dummy_texture = create_image_resource_texture(
            device,
            "tileink wgpu dummy image resource texture",
            1,
            1,
        );
        let image_resource_dummy_texture_view =
            image_resource_dummy_texture.create_view(&::wgpu::TextureViewDescriptor::default());
        Self {
            lines: WgpuBuffer::new(device, "tileink wgpu canvas lines"),
            path_records: WgpuBuffer::new(device, "tileink wgpu canvas path records"),
            draw_records: WgpuBuffer::new(device, "tileink wgpu canvas draw records"),
            sdf_blob: WgpuBuffer::new(device, "tileink wgpu canvas sdf blob"),
            sdf_shadow_blob: WgpuBuffer::new(device, "tileink wgpu canvas sdf shadow blob"),
            fine_paint_blob: WgpuBuffer::new(device, "tileink wgpu canvas fine paint blob"),
            scan_chunk_path_ids: WgpuBuffer::new(device, "tileink wgpu canvas scan chunk path ids"),
            scan_chunk_backdrop_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu canvas scan chunk backdrop offsets",
            ),
            scan_chunk_segment_starts: WgpuBuffer::new(
                device,
                "tileink wgpu canvas scan chunk segment starts",
            ),
            scan_chunk_lens: WgpuBuffer::new(device, "tileink wgpu canvas scan chunk lens"),
            scan_chunk_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu canvas scan chunk range starts",
            ),
            scan_chunk_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu canvas scan chunk range ends",
            ),
            cumsum_chunk_backdrop_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu canvas cumsum chunk backdrop offsets",
            ),
            cumsum_chunk_lens: WgpuBuffer::new(device, "tileink wgpu canvas cumsum chunk lens"),
            cumsum_row_chunk_starts: WgpuBuffer::new(
                device,
                "tileink wgpu canvas cumsum row chunk starts",
            ),
            cumsum_row_chunk_ends: WgpuBuffer::new(
                device,
                "tileink wgpu canvas cumsum row chunk ends",
            ),
            plan_layer_stack: WgpuBuffer::new(device, "tileink wgpu canvas plan layer stack"),
            brush_blob: WgpuBuffer::new(device, "tileink wgpu canvas brush blob"),
            image_resource_atlas,
            image_resource_atlas_view,
            image_resource_textures: Vec::new(),
            image_resource_texture_views: Vec::new(),
            image_resource_dummy_texture,
            image_resource_dummy_texture_view,
            image_resource_sampler: device.create_sampler(&::wgpu::SamplerDescriptor {
                label: Some("tileink wgpu image resource sampler"),
                address_mode_u: ::wgpu::AddressMode::ClampToEdge,
                address_mode_v: ::wgpu::AddressMode::ClampToEdge,
                address_mode_w: ::wgpu::AddressMode::ClampToEdge,
                mag_filter: ::wgpu::FilterMode::Linear,
                min_filter: ::wgpu::FilterMode::Linear,
                mipmap_filter: ::wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            }),
            image_resource_atlas_size: (1, 1, 1),
            text_runs: WgpuBuffer::new(device, "tileink wgpu canvas text runs"),
            coarse_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas coarse text blob"),
            fine_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas fine text blob"),
            glyph_atlas_signature: AtlasSignature::default(),
            fine_paint_sdf_shadow_base: 0,
            fine_paint_brush_base: 0,
            fine_text_image_base: 0,
            fine_text_image_data_base: 0,
        }
    }
}

pub(crate) fn create_image_resource_atlas_view(texture: &::wgpu::Texture) -> ::wgpu::TextureView {
    texture.create_view(&::wgpu::TextureViewDescriptor {
        label: Some("tileink wgpu image resource atlas view"),
        dimension: Some(::wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

pub(crate) fn create_image_resource_atlas_texture(
    device: &::wgpu::Device,
    width: u32,
    height: u32,
    layers: u32,
) -> ::wgpu::Texture {
    device.create_texture(&::wgpu::TextureDescriptor {
        label: Some("tileink wgpu image resource atlas"),
        size: ::wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::TEXTURE_BINDING | ::wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

pub(crate) fn create_image_resource_texture(
    device: &::wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
) -> ::wgpu::Texture {
    device.create_texture(&::wgpu::TextureDescriptor {
        label: Some(label),
        size: ::wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::TEXTURE_BINDING | ::wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
