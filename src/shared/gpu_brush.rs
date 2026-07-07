use crate::shared::gpu_layout::brush::{
    GPU_BRUSH_PATTERN, GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_U32_STRIDE,
};

use crate::shared::{
    brush::{Brush, ENCODED_BRUSH_HEADER_WORDS, push_encoded_brush},
    draw_record::DrawRecord,
    execution::ExecOp,
    image_resource::{AtlasRect, GpuImageResourceUpload, ImageResourceId},
    layer::{
        Layer,
        filter::{Filter, FilterPrimitiveKind},
    },
};

#[derive(Clone, Default)]
pub(crate) struct GpuBrushUpload {
    pub(crate) blob: Vec<u32>,
}

impl GpuBrushUpload {
    pub(crate) fn from_scene_brush_blob(
        draws: &[DrawRecord],
        brush_blob: &[u32],
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut upload = Self {
            blob: brush_blob.to_vec(),
        };
        for draw in draws {
            upload.patch_scene_resource_brush(draw, image_resources);
        }
        upload
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
        let (brush_offset, _) = push_encoded_brush(&mut self.blob, brush);
        debug_assert_eq!(offset, brush_offset);
        self.patch_resource_brush_at(offset, image_resources);
        offset
    }

    fn patch_scene_resource_brush(
        &mut self,
        draw: &DrawRecord,
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        if draw.brush_offset != DrawRecord::NONE {
            self.patch_resource_brush_at(draw.brush_offset, image_resources);
        }
    }

    fn patch_resource_brush_at(
        &mut self,
        brush_offset: u32,
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        let base = brush_offset as usize;
        if self.blob.get(base).copied() != Some(GPU_BRUSH_PATTERN_RESOURCE) {
            return;
        }
        let Some(data) = self.blob.get(base..base + GPU_BRUSH_U32_STRIDE) else {
            return;
        };
        let payload_start = base + data[2] as usize;
        let payload_len = data[3] as usize;
        let Some(payload) = self.blob.get(payload_start..payload_start + payload_len) else {
            self.clear_missing_resource_pattern(base);
            return;
        };
        if payload.len() < 3 {
            self.clear_missing_resource_pattern(base);
            return;
        }
        let id = ImageResourceId::decode(payload[0], payload[1], payload[2]);
        if let Some(rect) = image_resources.and_then(|resources| resources.image_rect(id)) {
            self.patch_atlas_resource_pattern(base, rect);
        } else {
            self.clear_missing_resource_pattern(base);
        }
    }

    fn patch_atlas_resource_pattern(&mut self, base: usize, rect: AtlasRect) {
        if rect.width > 0 && rect.height > 0 {
            self.blob[base + 2] = rect.x;
            self.blob[base + 3] = rect.y;
            self.blob[base + 5] = rect.width;
            self.blob[base + 6] = rect.height;
        } else {
            self.clear_missing_resource_pattern(base);
        }
    }

    fn clear_missing_resource_pattern(&mut self, base: usize) {
        self.blob[base] = GPU_BRUSH_PATTERN;
        self.blob[base + 2] = ENCODED_BRUSH_HEADER_WORDS as u32;
        self.blob[base + 3] = 0;
        self.blob[base + 5] = 0;
        self.blob[base + 6] = 0;
    }
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
