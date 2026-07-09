use crate::text::AtlasSignature;

use super::buffer::WgpuBuffer;

mod bindings;
mod upload;
mod work_buffers;

pub(crate) use bindings::{
    WgpuCoarseBindings, WgpuCumsumBindings, WgpuFilterBindings, WgpuScanBindings,
    WgpuTileFineBindings,
};
pub(crate) use upload::WgpuSceneUploadStaging;
pub(crate) use work_buffers::{WgpuCoarseBuffers, WgpuScanBuffers};
pub(crate) struct WgpuSceneBuffers {
    lines: WgpuBuffer,
    path_records: WgpuBuffer,
    draw_records: WgpuBuffer,
    sdf_blob: WgpuBuffer,
    sdf_shadow_blob: WgpuBuffer,
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
    image_resource_metadata: WgpuBuffer,
    image_resource_pixels: WgpuBuffer,
    text_runs: WgpuBuffer,
    coarse_text_blob: WgpuBuffer,
    fine_text_blob: WgpuBuffer,
    glyph_atlas_signature: AtlasSignature,
    fine_text_image_base: u32,
    fine_text_image_data_base: u32,
}

impl WgpuSceneBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            lines: WgpuBuffer::new(device, "tileink wgpu canvas lines"),
            path_records: WgpuBuffer::new(device, "tileink wgpu canvas path records"),
            draw_records: WgpuBuffer::new(device, "tileink wgpu canvas draw records"),
            sdf_blob: WgpuBuffer::new(device, "tileink wgpu canvas sdf blob"),
            sdf_shadow_blob: WgpuBuffer::new(device, "tileink wgpu canvas sdf shadow blob"),
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
            image_resource_metadata: WgpuBuffer::new(
                device,
                "tileink wgpu image resource metadata",
            ),
            image_resource_pixels: WgpuBuffer::new(device, "tileink wgpu image resource pixels"),
            text_runs: WgpuBuffer::new(device, "tileink wgpu canvas text runs"),
            coarse_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas coarse text blob"),
            fine_text_blob: WgpuBuffer::new(device, "tileink wgpu canvas fine text blob"),
            glyph_atlas_signature: AtlasSignature::default(),
            fine_text_image_base: 0,
            fine_text_image_data_base: 0,
        }
    }
}
