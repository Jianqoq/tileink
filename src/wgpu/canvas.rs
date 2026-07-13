use crate::text::AtlasSignature;
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::buffer::{WgpuBuffer, WgpuRangeScatter, WgpuRangeScatterPipeline};

mod bindings;
mod upload;
mod work_buffers;

pub(crate) use bindings::{
    WgpuCoarseBindings, WgpuCumsumBindings, WgpuFilterBindings, WgpuImageResourceBindingKey,
    WgpuImageResourceBindings, WgpuScanBindings, WgpuTileFineBindings,
};
pub(crate) use upload::WgpuSceneUploadStaging;
pub(crate) use work_buffers::{WgpuCoarseBindGroups, WgpuCoarseBuffers, WgpuScanBuffers};
static NEXT_SCENE_BUFFERS_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct WgpuSceneBuffers {
    id: u64,
    lines: WgpuBuffer,
    path_records: WgpuBuffer,
    draw_records: WgpuBuffer,
    draw_batch_ids: WgpuBuffer,
    paint_blob: WgpuBuffer,
    scan_chunks: WgpuBuffer,
    scan_chunk_ranges: WgpuBuffer,
    cumsum_chunk_backdrop_offsets: WgpuBuffer,
    cumsum_chunk_lens: WgpuBuffer,
    cumsum_row_chunk_starts: WgpuBuffer,
    cumsum_row_chunk_ends: WgpuBuffer,
    plan_layer_stack: WgpuBuffer,
    image_resource_atlas: ::wgpu::Texture,
    image_resource_atlas_view: ::wgpu::TextureView,
    // Standalone GPU textures for large image resources that bypass atlas pages.
    image_resource_textures: Vec<::wgpu::Texture>,
    // Texture views bound through the large-image texture table.
    image_resource_texture_views: Vec<::wgpu::TextureView>,
    // 1x1 fallback texture used to pad unused texture-table slots.
    _image_resource_dummy_texture: ::wgpu::Texture,
    // View for the dummy texture, shared by fallback and table padding bindings.
    image_resource_dummy_texture_view: ::wgpu::TextureView,
    image_resource_sampler: ::wgpu::Sampler,
    image_resource_atlas_size: (u32, u32, u32),
    image_resource_binding_generation: u64,
    text_runs: WgpuBuffer,
    coarse_text_blob: WgpuBuffer,
    fine_text_blob: WgpuBuffer,
    glyph_atlas_signature: AtlasSignature,
    paint_sdf_shadow_base: u32,
    paint_brush_base: u32,
    fine_text_image_base: u32,
    fine_text_image_data_base: u32,
    range_scatter: WgpuRangeScatter,
}

impl WgpuSceneBuffers {
    pub(crate) fn new(
        device: &::wgpu::Device,
        range_scatter_pipeline: Rc<WgpuRangeScatterPipeline>,
    ) -> Self {
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
            id: NEXT_SCENE_BUFFERS_ID.fetch_add(1, Ordering::Relaxed),
            lines: WgpuBuffer::new(device, "tileink wgpu canvas lines"),
            path_records: WgpuBuffer::new(device, "tileink wgpu canvas path records"),
            draw_records: WgpuBuffer::new(device, "tileink wgpu canvas draw records"),
            draw_batch_ids: WgpuBuffer::new(device, "tileink wgpu canvas draw batch ids"),
            paint_blob: WgpuBuffer::new(device, "tileink wgpu canvas paint blob"),
            scan_chunks: WgpuBuffer::new(device, "tileink wgpu canvas scan chunks"),
            scan_chunk_ranges: WgpuBuffer::new(device, "tileink wgpu canvas scan chunk ranges"),
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
            image_resource_atlas,
            image_resource_atlas_view,
            image_resource_textures: Vec::new(),
            image_resource_texture_views: Vec::new(),
            _image_resource_dummy_texture: image_resource_dummy_texture,
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
            image_resource_binding_generation: 1,
            text_runs: WgpuBuffer::new(device, "tileink wgpu canvas text runs"),
            coarse_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas coarse text blob"),
            fine_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas fine text blob"),
            glyph_atlas_signature: AtlasSignature::default(),
            paint_sdf_shadow_base: 0,
            paint_brush_base: 0,
            fine_text_image_base: 0,
            fine_text_image_data_base: 0,
            range_scatter: WgpuRangeScatter::new(range_scatter_pipeline),
        }
    }

    #[cfg(test)]
    pub(crate) fn upload_test_batch_ids(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        ids: &[u32],
    ) {
        self.draw_batch_ids
            .upload(device, queue, "tileink wgpu test draw batch ids", ids);
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
