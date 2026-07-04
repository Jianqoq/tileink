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
    tile_draw_range_starts: WgpuBuffer,
    tile_draw_range_ends: WgpuBuffer,
    tile_draw_indices: WgpuBuffer,
    plan_layer_stack_tags: WgpuBuffer,
    plan_layer_stack_draws: WgpuBuffer,
    plan_layer_stack_payloads: WgpuBuffer,
    draw_brush_data: WgpuBuffer,
    draw_brush_params: WgpuBuffer,
    draw_brush_payloads: WgpuBuffer,
    image_resource_metadata: WgpuBuffer,
    image_resource_pixels: WgpuBuffer,
    text_run_starts: WgpuBuffer,
    text_run_counts: WgpuBuffer,
    glyph_image_ids: WgpuBuffer,
    glyph_x: WgpuBuffer,
    glyph_y: WgpuBuffer,
    glyph_image_left: WgpuBuffer,
    glyph_image_top: WgpuBuffer,
    glyph_image_width: WgpuBuffer,
    glyph_image_height: WgpuBuffer,
    glyph_image_content: WgpuBuffer,
    glyph_image_data_offsets: WgpuBuffer,
    glyph_image_data: WgpuBuffer,
    glyph_atlas_signature: AtlasSignature,
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
            tile_draw_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu canvas tile draw range starts",
            ),
            tile_draw_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu canvas tile draw range ends",
            ),
            tile_draw_indices: WgpuBuffer::new(device, "tileink wgpu canvas tile draw indices"),
            plan_layer_stack_tags: WgpuBuffer::new(
                device,
                "tileink wgpu canvas plan layer stack tags",
            ),
            plan_layer_stack_draws: WgpuBuffer::new(
                device,
                "tileink wgpu canvas plan layer stack draws",
            ),
            plan_layer_stack_payloads: WgpuBuffer::new(
                device,
                "tileink wgpu canvas plan layer stack payloads",
            ),
            draw_brush_data: WgpuBuffer::new(device, "tileink wgpu canvas draw brush data"),
            draw_brush_params: WgpuBuffer::new(device, "tileink wgpu canvas draw brush params"),
            draw_brush_payloads: WgpuBuffer::new(device, "tileink wgpu canvas draw brush payloads"),
            image_resource_metadata: WgpuBuffer::new(
                device,
                "tileink wgpu image resource metadata",
            ),
            image_resource_pixels: WgpuBuffer::new(device, "tileink wgpu image resource pixels"),
            text_run_starts: WgpuBuffer::new(device, "tileink wgpu canvas text run starts"),
            text_run_counts: WgpuBuffer::new(device, "tileink wgpu canvas text run counts"),
            glyph_image_ids: WgpuBuffer::new(device, "tileink wgpu canvas glyph image ids"),
            glyph_x: WgpuBuffer::new(device, "tileink wgpu canvas glyph x"),
            glyph_y: WgpuBuffer::new(device, "tileink wgpu canvas glyph y"),
            glyph_image_left: WgpuBuffer::new(device, "tileink wgpu canvas glyph image left"),
            glyph_image_top: WgpuBuffer::new(device, "tileink wgpu canvas glyph image top"),
            glyph_image_width: WgpuBuffer::new(device, "tileink wgpu canvas glyph image width"),
            glyph_image_height: WgpuBuffer::new(device, "tileink wgpu canvas glyph image height"),
            glyph_image_content: WgpuBuffer::new(device, "tileink wgpu canvas glyph image content"),
            glyph_image_data_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu canvas glyph image data offsets",
            ),
            glyph_image_data: WgpuBuffer::new(device, "tileink wgpu canvas glyph image data"),
            glyph_atlas_signature: AtlasSignature::default(),
        }
    }
}
