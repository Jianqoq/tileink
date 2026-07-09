use crate::{
    canvas::Canvas,
    shared::{
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_brush::GpuBrushUpload,
        gpu_coarse::LayerStackRecord,
        gpu_coarse::{
            coarse_work_tile_draw_index_word_offset, coarse_work_tile_draw_record_word_offset,
        },
        gpu_plan::{
            GpuBufferLengths, GpuCumsumPlan, GpuScanChunk, GpuScanChunkRange, TileDrawBins,
            build_cumsum_plan_into, build_scan_chunks_into,
        },
        gpu_text::{GlyphImageRecord, GlyphRecord, GlyphRunRecord, text_blob_word_len},
        gpu_types::{
            GPU_GLYPH_COLOR, GPU_GLYPH_LINEAR_COLOR, GPU_GLYPH_LINEAR_MASK,
            GPU_GLYPH_LINEAR_SUBPIXEL_MASK, GPU_GLYPH_MASK, GPU_GLYPH_SUBPIXEL_MASK,
            GPU_LAYER_BLEND, GPU_LAYER_CLIP, GPU_LAYER_OPACITY,
        },
        image::rgba8_pack,
        image_resource::GpuImageResourceUpload,
        pixel::{mul_div255, opacity_f32_to_u8},
    },
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};

use super::super::buffer::WgpuBuffer;
use super::super::profile::profile_cpu;
use super::{
    WgpuCoarseBuffers, WgpuSceneBuffers, create_image_resource_atlas_texture,
    create_image_resource_atlas_view, create_image_resource_texture,
};

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    u32s: Vec<u32>,
    draw_records: Vec<DrawRecord>,
    text: TextUpload,
    scan_chunks: Vec<GpuScanChunk>,
    scan_chunk_ranges: Vec<GpuScanChunkRange>,
    cumsum_plan: GpuCumsumPlan,
    tile_draw_bins: TileDrawBins,
    tile_draw_cursors: Vec<u32>,
    layer_stack: Vec<LayerStackRecord>,
    scene_brush_blob: Vec<u32>,
    fine_text_blob: Vec<u32>,
}

impl WgpuSceneUploadStaging {
    pub(crate) fn build_lengths(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
    ) -> GpuBufferLengths {
        // Lengths and tile draw bins must describe the same scene; building both here avoids
        // recounting every draw/tile intersection later in prepare.
        GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            canvas,
            text,
            &mut self.tile_draw_bins,
            &mut self.tile_draw_cursors,
        )
    }
}

#[derive(Default)]
struct TextUpload {
    runs: Vec<GlyphRunRecord>,
    glyphs: Vec<GlyphRecord>,
    images: Vec<GlyphImageRecord>,
    image_data: Vec<u32>,
    atlas_signature: AtlasSignature,
    atlas_dirty: bool,
}

impl TextUpload {
    fn refill(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        current_atlas_signature: AtlasSignature,
    ) {
        self.clear();
        let Some(text) = text else {
            return;
        };

        self.runs
            .extend(canvas.text_runs.iter().map(|run| GlyphRunRecord {
                glyph_start: run.glyph_start,
                glyph_count: run.glyph_count,
            }));
        self.glyphs.reserve(canvas.text_glyphs.len());
        for glyph in &canvas.text_glyphs {
            self.glyphs.push(GlyphRecord {
                image_id: text
                    .image_id_for_cache_key(glyph.cache_key)
                    .unwrap_or(u32::MAX),
                x: glyph.x,
                y: glyph.y,
            });
        }

        let atlas_signature = text.atlas_signature();
        self.atlas_dirty = atlas_signature != current_atlas_signature;
        self.atlas_signature = atlas_signature;
        for image in text.images() {
            let data_offset = self.image_data.len() as u32;
            let content = match image.content {
                PreparedGlyphContent::Mask => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_MASK,
                    };
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                    content
                }
                PreparedGlyphContent::Color => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_COLOR,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_COLOR,
                    };
                    for pixel in image.data.chunks_exact(4) {
                        let a = pixel[3];
                        self.image_data.push(rgba8_pack([
                            mul_div255(pixel[0], a),
                            mul_div255(pixel[1], a),
                            mul_div255(pixel[2], a),
                            a,
                        ]));
                    }
                    content
                }
                PreparedGlyphContent::SubpixelMask => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_SUBPIXEL_MASK,
                    };
                    for pixel in image.data.chunks_exact(3) {
                        self.image_data
                            .push(rgba8_pack([pixel[0], pixel[1], pixel[2], 0]));
                    }
                    content
                }
            };
            self.images.push(GlyphImageRecord {
                left: image.left,
                top: image.top,
                width: image.width,
                height: image.height,
                content,
                data_offset,
            });
        }
    }

    fn clear(&mut self) {
        self.runs.clear();
        self.glyphs.clear();
        self.images.clear();
        self.image_data.clear();
        self.atlas_signature = AtlasSignature::default();
        self.atlas_dirty = false;
    }
}

fn word_offset(words: usize) -> ::wgpu::BufferAddress {
    words as ::wgpu::BufferAddress * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
}

fn upload_coarse_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    scratch: &mut Vec<u32>,
    text: &TextUpload,
) {
    scratch.clear();
    scratch.reserve(text_blob_word_len(
        text.runs.len(),
        text.glyphs.len(),
        text.images.len(),
    ));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.runs));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.glyphs));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.images));
    buffer.upload(
        device,
        queue,
        "tileink wgpu canvas coarse text blob",
        scratch,
    );
}

fn upload_fine_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    blob: &mut Vec<u32>,
    text: &TextUpload,
) -> (u32, u32) {
    let image_base = text.glyphs.len() * std::mem::size_of::<GlyphRecord>() / 4;
    let image_data_base =
        image_base + text.images.len() * std::mem::size_of::<GlyphImageRecord>() / 4;
    blob.clear();
    blob.reserve(image_data_base + text.image_data.len());
    blob.extend_from_slice(bytemuck::cast_slice(&text.glyphs));
    blob.extend_from_slice(bytemuck::cast_slice(&text.images));
    blob.extend_from_slice(&text.image_data);
    buffer.upload(device, queue, "tileink wgpu canvas fine text blob", blob);
    (image_base as u32, image_data_base as u32)
}

fn encode_layer_payload(entry: LayerStackEntry) -> u32 {
    match entry {
        LayerStackEntry::Clip { .. } => 0,
        LayerStackEntry::Opacity { opacity, .. } => opacity_f32_to_u8(opacity) as u32,
        LayerStackEntry::Blend { mode, .. } => mode.mix as u32 | ((mode.compose as u32) << 8),
    }
}

impl WgpuSceneBuffers {
    pub(crate) fn upload_image_resources(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: &GpuImageResourceUpload,
        force_all: bool,
    ) {
        let page_size = upload.atlas_page_size().max(1);
        let page_count = upload.atlas_page_count().max(1);
        let atlas_capacity = grow_image_resource_atlas_capacity(
            self.image_resource_atlas_size,
            (page_size, page_size, page_count),
            device.limits().max_texture_dimension_2d,
            device.limits().max_texture_array_layers,
        );
        if self.image_resource_atlas_size != atlas_capacity {
            self.image_resource_atlas = create_image_resource_atlas_texture(
                device,
                atlas_capacity.0,
                atlas_capacity.1,
                atlas_capacity.2,
            );
            self.image_resource_atlas_view =
                create_image_resource_atlas_view(&self.image_resource_atlas);
            self.image_resource_atlas_size = atlas_capacity;
        }
        for page in upload.atlas_pages() {
            if !force_all && !page.dirty {
                continue;
            }
            if page.pixels.is_empty() {
                continue;
            }
            queue.write_texture(
                ::wgpu::TexelCopyTextureInfo {
                    texture: &self.image_resource_atlas,
                    mip_level: 0,
                    origin: ::wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: page.index,
                    },
                    aspect: ::wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&page.pixels),
                ::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(page.size * std::mem::size_of::<u32>() as u32),
                    rows_per_image: Some(page.size),
                },
                ::wgpu::Extent3d {
                    width: page.size,
                    height: page.size,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.upload_image_resource_textures(device, queue, upload, force_all);
    }

    fn upload_image_resource_textures(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: &GpuImageResourceUpload,
        force_all: bool,
    ) {
        for texture in upload.textures() {
            let index = texture.index as usize;
            let recreate = self
                .image_resource_textures
                .get(index)
                .is_none_or(|existing| {
                    let size = existing.size();
                    size.width != texture.width || size.height != texture.height
                });
            if recreate {
                while self.image_resource_textures.len() <= index {
                    self.image_resource_textures
                        .push(create_image_resource_texture(
                            device,
                            "tileink wgpu image resource texture",
                            1,
                            1,
                        ));
                    self.image_resource_texture_views.push(
                        self.image_resource_textures
                            .last()
                            .expect("pushed texture")
                            .create_view(&::wgpu::TextureViewDescriptor::default()),
                    );
                }
                self.image_resource_textures[index] = create_image_resource_texture(
                    device,
                    "tileink wgpu image resource texture",
                    texture.width,
                    texture.height,
                );
                self.image_resource_texture_views[index] = self.image_resource_textures[index]
                    .create_view(&::wgpu::TextureViewDescriptor::default());
            }
            if force_all || texture.dirty || recreate {
                queue.write_texture(
                    self.image_resource_textures[index].as_image_copy(),
                    bytemuck::cast_slice(&texture.pixels),
                    ::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(texture.width * std::mem::size_of::<u32>() as u32),
                        rows_per_image: Some(texture.height),
                    },
                    ::wgpu::Extent3d {
                        width: texture.width,
                        height: texture.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        lengths: GpuBufferLengths,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        // Keep canvas upload profiling split between CPU-side plan construction and queue uploads.
        profile_cpu("prepare.upload_scene.build_scan_chunks", || {
            build_scan_chunks_into(
                canvas,
                lengths.scan_chunk_count,
                &mut staging.scan_chunks,
                &mut staging.scan_chunk_ranges,
            );
        });
        profile_cpu("prepare.upload_scene.build_cumsum_plan", || {
            build_cumsum_plan_into(canvas, lengths, &mut staging.cumsum_plan);
        });
        profile_cpu("prepare.upload_scene.upload_draw_records", || {
            self.upload_draw_records(device, queue, &canvas.draw_records, text.is_some(), staging);
        });
        profile_cpu("prepare.upload_scene.upload_scene_records", || {
            self.upload_scene_records(device, queue, canvas, image_resources, staging);
        });
        profile_cpu("prepare.upload_scene.upload_layer_stack", || {
            self.upload_plan_layer_stack(device, queue, &plan.layer_stack_data, staging);
        });
        profile_cpu("prepare.upload_scene.upload_text", || {
            self.upload_text(device, queue, canvas, text, staging);
        });
        profile_cpu("prepare.upload_scene.upload_scan_plan", || {
            self.upload_scan_plan(device, queue, staging);
        });
        profile_cpu("prepare.upload_scene.upload_cumsum_plan", || {
            self.upload_cumsum_plan(device, queue, staging);
        });
    }

    fn upload_scene_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        profile_cpu("prepare.upload_scene.records.geometry", || {
            self.lines
                .upload(device, queue, "tileink wgpu canvas lines", &canvas.lines);
            self.path_records.upload(
                device,
                queue,
                "tileink wgpu canvas path records",
                &canvas.path_records,
            );
        });
        profile_cpu("prepare.upload_scene.upload_paint_blob", || {
            let scene_brush_blob = if GpuBrushUpload::scene_brushes_need_resource_patch(
                &canvas.draw_records,
                &canvas.brush_blob,
            ) {
                staging.scene_brush_blob.clear();
                staging
                    .scene_brush_blob
                    .extend_from_slice(&canvas.brush_blob);
                GpuBrushUpload::patch_scene_brush_blob(
                    &mut staging.scene_brush_blob,
                    &canvas.draw_records,
                    image_resources,
                );
                &staging.scene_brush_blob
            } else {
                &canvas.brush_blob
            };

            self.paint_sdf_shadow_base = canvas.sdf_blob.len() as u32;
            self.paint_brush_base = (canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len()) as u32;
            let paint_words =
                canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len() + scene_brush_blob.len();
            // SDF, SDF-shadow, and scene brushes share one storage buffer so coarse, fine,
            // and filter bind the same paint data without CPU-side repacking.
            self.paint_blob.resize_uninit::<u32>(
                device,
                "tileink wgpu canvas paint blob",
                paint_words,
            );
            self.paint_blob.write_at(queue, 0, &canvas.sdf_blob);
            self.paint_blob.write_at(
                queue,
                word_offset(self.paint_sdf_shadow_base as usize),
                &canvas.sdf_shadow_blob,
            );
            self.paint_blob.write_at(
                queue,
                word_offset(self.paint_brush_base as usize),
                scene_brush_blob,
            );
        });
    }

    fn upload_draw_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        draw_records: &[DrawRecord],
        text_enabled: bool,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        if text_enabled {
            self.draw_records.upload(
                device,
                queue,
                "tileink wgpu canvas draw records",
                draw_records,
            );
            return;
        }

        staging.draw_records.clear();
        staging.draw_records.extend_from_slice(draw_records);
        for draw in &mut staging.draw_records {
            draw.glyph_run_id = DrawRecord::NONE;
        }
        self.draw_records.upload(
            device,
            queue,
            "tileink wgpu canvas draw records",
            &staging.draw_records,
        );
    }

    fn upload_plan_layer_stack(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        layer_stack: &[LayerStackEntry],
        staging: &mut WgpuSceneUploadStaging,
    ) {
        staging.layer_stack.clear();
        staging.layer_stack.reserve(layer_stack.len());
        staging
            .layer_stack
            .extend(layer_stack.iter().map(|entry| LayerStackRecord {
                tag: match entry {
                    LayerStackEntry::Clip { .. } => GPU_LAYER_CLIP,
                    LayerStackEntry::Opacity { .. } => GPU_LAYER_OPACITY,
                    LayerStackEntry::Blend { .. } => GPU_LAYER_BLEND,
                },
                draw: match *entry {
                    LayerStackEntry::Clip { draw }
                    | LayerStackEntry::Opacity { draw, .. }
                    | LayerStackEntry::Blend { draw, .. } => draw,
                },
                payload: encode_layer_payload(*entry),
            }));
        self.plan_layer_stack.upload(
            device,
            queue,
            "tileink wgpu canvas plan layer stack",
            &staging.layer_stack,
        );
    }

    fn upload_text(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        profile_cpu("prepare.upload_scene.text.refill", || {
            staging
                .text
                .refill(canvas, text, self.glyph_atlas_signature);
        });
        profile_cpu("prepare.upload_scene.text.runs", || {
            self.text_runs.upload(
                device,
                queue,
                "tileink wgpu canvas text runs",
                &staging.text.runs,
            );
            upload_coarse_text_blob(
                device,
                queue,
                &mut self.coarse_text_blob,
                &mut staging.u32s,
                &staging.text,
            );
            let (image_base, image_data_base) = upload_fine_text_blob(
                device,
                queue,
                &mut self.fine_text_blob,
                &mut staging.fine_text_blob,
                &staging.text,
            );
            self.fine_text_image_base = image_base;
            self.fine_text_image_data_base = image_data_base;
        });
        if text.is_none() {
            self.glyph_atlas_signature = AtlasSignature::default();
            return;
        }
        if !staging.text.atlas_dirty {
            return;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
    }

    fn upload_scan_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        // Scan kernels consume the CPU-built AoS plan directly, avoiding per-field packing in prepare.
        self.scan_chunks.upload(
            device,
            queue,
            "tileink wgpu canvas scan chunks",
            &staging.scan_chunks,
        );
        self.scan_chunk_ranges.upload(
            device,
            queue,
            "tileink wgpu canvas scan chunk ranges",
            &staging.scan_chunk_ranges,
        );
    }

    fn upload_cumsum_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        staging: &WgpuSceneUploadStaging,
    ) {
        self.cumsum_chunk_backdrop_offsets.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum chunk backdrop offsets",
            &staging.cumsum_plan.chunk_backdrop_offsets,
        );
        self.cumsum_chunk_lens.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum chunk lens",
            &staging.cumsum_plan.chunk_lens,
        );
        self.cumsum_row_chunk_starts.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum row chunk starts",
            &staging.cumsum_plan.row_chunk_starts,
        );
        self.cumsum_row_chunk_ends.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum row chunk ends",
            &staging.cumsum_plan.row_chunk_ends,
        );
    }
}

fn grow_image_resource_atlas_capacity(
    current: (u32, u32, u32),
    required: (u32, u32, u32),
    max_dimension: u32,
    max_layers: u32,
) -> (u32, u32, u32) {
    (
        grow_image_resource_atlas_axis(current.0, required.0, max_dimension),
        grow_image_resource_atlas_axis(current.1, required.1, max_dimension),
        grow_image_resource_atlas_axis(current.2, required.2, max_layers),
    )
}

fn grow_image_resource_atlas_axis(current: u32, required: u32, max_dimension: u32) -> u32 {
    let required = required.max(1);
    let max_dimension = max_dimension.max(1);
    let mut capacity = current.max(1).min(max_dimension);
    while capacity < required {
        let doubled = capacity.saturating_mul(2).min(max_dimension);
        if doubled <= capacity {
            capacity = required.min(max_dimension);
            break;
        }
        capacity = doubled;
    }
    capacity
}

#[cfg(test)]
mod tests {
    use super::grow_image_resource_atlas_capacity;

    #[test]
    fn image_resource_atlas_capacity_reuses_existing_texture_when_it_fits() {
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (128, 64, 2), 4096, 256),
            (256, 128, 4)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (256, 128, 4), 4096, 256),
            (256, 128, 4)
        );
    }

    #[test]
    fn image_resource_atlas_capacity_grows_by_doubling_until_required_size_fits() {
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (257, 129, 5), 4096, 256),
            (512, 256, 8)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (900, 129, 9), 4096, 256),
            (1024, 256, 16)
        );
    }

    #[test]
    fn image_resource_atlas_capacity_respects_device_limit() {
        assert_eq!(
            grow_image_resource_atlas_capacity((4096, 4096, 128), (5000, 5000, 300), 6000, 256),
            (6000, 6000, 256)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((1, 1, 1), (0, 0, 0), 4096, 256),
            (1, 1, 1)
        );
    }
}

impl WgpuCoarseBuffers {
    pub(crate) fn upload_tile_draw_bins(
        &self,
        queue: &::wgpu::Queue,
        lengths: crate::shared::gpu_plan::GpuBufferLengths,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        let record_word_offset = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let index_word_offset = coarse_work_tile_draw_index_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let bins = &staging.tile_draw_bins;
        // Coarse work keeps tile draw records and indices in adjacent regions; write each source directly.
        self.work
            .write_at(queue, word_offset(record_word_offset), &bins.records);
        self.work
            .write_at(queue, word_offset(index_word_offset), &bins.draw_indices);
    }
}
