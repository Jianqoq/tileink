use crate::shared::gpu_layout::brush::{
    GPU_BRUSH_PATTERN, GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_U32_STRIDE,
    GPU_RESOURCE_TEXTURE_PLACEMENT_BIT,
};

use crate::shared::{
    brush::{Brush, ENCODED_BRUSH_HEADER_WORDS, push_encoded_brush},
    draw_record::DrawRecord,
    execution::ExecOp,
    image_resource::{GpuImageResourceUpload, ImageResourceId, ImageResourcePlacement},
    layer::{
        Layer,
        filter::{Filter, FilterPrimitiveKind},
    },
};

#[derive(Clone, Default)]
pub(crate) struct GpuBrushUpload {
    pub(crate) blob: Vec<u32>,
    pub(crate) offsets: Vec<u32>,
}

impl GpuBrushUpload {
    pub(crate) fn draw_uses_resource_brush(draw: &DrawRecord, brush_blob: &[u32]) -> bool {
        let base = draw.brush_offset as usize;
        draw.brush_offset != DrawRecord::NONE && brush_blob[base] == GPU_BRUSH_PATTERN_RESOURCE
    }

    /// Resolve resource headers from immutable CPU records, never from previously patched
    /// GPU headers: words 2/3 change from payload offset/length to atlas coordinates.
    pub(crate) fn patch_brush_record(
        destination: &mut [u32],
        source: &[u32],
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        if let Some(header) = resource_brush_header(source, image_resources) {
            destination[..GPU_BRUSH_U32_STRIDE].copy_from_slice(&header);
        }
    }

    pub(crate) fn patch_scene_brush_blob(
        destination: &mut [u32],
        source: &[u32],
        draws: &[DrawRecord],
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        for draw in draws {
            if draw.brush_offset != DrawRecord::NONE {
                let range = draw.brush_offset as usize
                    ..draw.brush_offset as usize + draw.brush_len as usize;
                Self::patch_brush_record(
                    &mut destination[range.clone()],
                    &source[range],
                    image_resources,
                );
            }
        }
    }

    pub(crate) fn patch_scene_brush_blob_draw_ranges(
        destination: &mut [u32],
        source: &[u32],
        draws: &[DrawRecord],
        ranges: &[std::ops::Range<usize>],
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        for range in ranges {
            Self::patch_scene_brush_blob(
                destination,
                source,
                &draws[range.clone()],
                image_resources,
            );
        }
    }

    pub(crate) fn from_filter_plan_with_resources(
        ops: &[ExecOp],
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut upload = Self::default();
        collect_filter_brushes_for_ops(ops, &mut upload, image_resources);
        upload
    }

    pub(crate) fn from_filter_ops_and_filter_with_resources(
        ops: &[ExecOp],
        filter: &Filter,
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut upload = Self::default();
        collect_filter_brushes_for_ops(ops, &mut upload, image_resources);
        collect_filter_brush(filter, &mut upload, image_resources);
        upload
    }

    pub(crate) fn push_brush_with_resources(
        &mut self,
        brush: &Brush,
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> u32 {
        let offset = self.blob.len() as u32;
        self.offsets.push(offset);
        let (brush_offset, _) = push_encoded_brush(&mut self.blob, brush);
        debug_assert_eq!(offset, brush_offset);
        // This newly encoded record is still in CPU format; resolve before overwriting it.
        if let Some(header) = resource_brush_header(&self.blob[offset as usize..], image_resources)
        {
            self.blob[offset as usize..offset as usize + GPU_BRUSH_U32_STRIDE]
                .copy_from_slice(&header);
        }
        offset
    }
}

fn resource_brush_header(
    source: &[u32],
    image_resources: Option<&GpuImageResourceUpload>,
) -> Option<[u32; GPU_BRUSH_U32_STRIDE]> {
    if source[0] != GPU_BRUSH_PATTERN_RESOURCE {
        return None;
    }
    // Canvas and filter brush encoders own this internal format. Resource payloads
    // contain exactly one three-word image ID; malformed records are invariant errors.
    let payload = &source[source[2] as usize..];
    let id = ImageResourceId::decode(payload[0], payload[1], payload[2])
        .expect("resource brush scope is encoder-owned");
    let mut header = std::array::from_fn(|index| source[index]);
    match image_resources.and_then(|resources| resources.image_placement(id)) {
        Some(ImageResourcePlacement::Atlas(rect)) if rect.width > 0 && rect.height > 0 => {
            header[2..7].copy_from_slice(&[rect.x, rect.y, rect.page, rect.width, rect.height]);
        }
        Some(ImageResourcePlacement::Texture(rect)) if rect.width > 0 && rect.height > 0 => {
            // The high bit selects a texture-table index instead of an atlas page.
            header[2..7].copy_from_slice(&[
                0,
                0,
                GPU_RESOURCE_TEXTURE_PLACEMENT_BIT | rect.index,
                rect.width,
                rect.height,
            ]);
        }
        _ => {
            // A genuinely absent or empty image has transparent pattern semantics.
            header[0] = GPU_BRUSH_PATTERN;
            header[2..7].copy_from_slice(&[ENCODED_BRUSH_HEADER_WORDS as u32, 0, 0, 0, 0]);
        }
    }
    Some(header)
}

fn collect_filter_brushes_for_ops(
    ops: &[ExecOp],
    upload: &mut GpuBrushUpload,
    image_resources: Option<&GpuImageResourceUpload>,
) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_brushes_for_ops(children, upload, image_resources);
                    collect_filter_brush(filter, upload, image_resources);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_brush(filter, upload, image_resources);
                    collect_filter_brushes_for_ops(children, upload, image_resources);
                }
                _ => collect_filter_brushes_for_ops(children, upload, image_resources),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_brushes_for_ops(content, upload, image_resources);
                collect_filter_brushes_for_ops(mask, upload, image_resources);
            }
            _ => {}
        }
    }
}

fn collect_filter_brush(
    filter: &Filter,
    upload: &mut GpuBrushUpload,
    image_resources: Option<&GpuImageResourceUpload>,
) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_brush(filter, upload, image_resources);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                match &primitive.kind {
                    FilterPrimitiveKind::Filter(filter) => {
                        collect_filter_brush(filter, upload, image_resources)
                    }
                    FilterPrimitiveKind::Image { brush } => {
                        upload.push_brush_with_resources(brush, image_resources);
                    }
                    _ => {}
                }
            }
        }
        Filter::DropShadow { brush, .. } => {
            upload.push_brush_with_resources(brush, image_resources);
        }
        Filter::Flood { brush } => {
            upload.push_brush_with_resources(brush, image_resources);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
