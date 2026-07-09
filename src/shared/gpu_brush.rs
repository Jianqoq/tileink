use crate::shared::gpu_layout::brush::{
    GPU_BRUSH_PATTERN, GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_U32_STRIDE,
    GPU_RESOURCE_TEXTURE_PLACEMENT_BIT,
};

use crate::shared::{
    brush::{Brush, ENCODED_BRUSH_HEADER_WORDS, push_encoded_brush},
    draw_record::DrawRecord,
    execution::ExecOp,
    image_resource::{AtlasRect, GpuImageResourceUpload, ImageResourceId, ImageResourcePlacement},
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
    pub(crate) fn scene_brushes_need_resource_patch(
        draws: &[DrawRecord],
        brush_blob: &[u32],
    ) -> bool {
        draws.iter().any(|draw| {
            let base = draw.brush_offset as usize;
            draw.brush_offset != DrawRecord::NONE
                && brush_blob.get(base).copied() == Some(GPU_BRUSH_PATTERN_RESOURCE)
        })
    }

    pub(crate) fn patch_scene_brush_blob(
        blob: &mut [u32],
        draws: &[DrawRecord],
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        for draw in draws {
            if draw.brush_offset != DrawRecord::NONE {
                patch_resource_brush_at(blob, draw.brush_offset, image_resources);
            }
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
        let (brush_offset, _) = push_encoded_brush(&mut self.blob, brush);
        debug_assert_eq!(offset, brush_offset);
        patch_resource_brush_at(&mut self.blob, offset, image_resources);
        offset
    }
}

fn patch_resource_brush_at(
    blob: &mut [u32],
    brush_offset: u32,
    image_resources: Option<&GpuImageResourceUpload>,
) {
    let base = brush_offset as usize;
    if blob.get(base).copied() != Some(GPU_BRUSH_PATTERN_RESOURCE) {
        return;
    }
    let Some(data) = blob.get(base..base + GPU_BRUSH_U32_STRIDE) else {
        return;
    };
    let payload_start = base + data[2] as usize;
    let payload_len = data[3] as usize;
    let Some(payload) = blob.get(payload_start..payload_start + payload_len) else {
        clear_missing_resource_pattern(blob, base);
        return;
    };
    if payload.len() < 3 {
        clear_missing_resource_pattern(blob, base);
        return;
    }
    let id = ImageResourceId::decode(payload[0], payload[1], payload[2]);
    if let Some(placement) = image_resources.and_then(|resources| resources.image_placement(id)) {
        patch_resource_pattern(blob, base, placement);
    } else {
        clear_missing_resource_pattern(blob, base);
    }
}

fn patch_resource_pattern(blob: &mut [u32], base: usize, placement: ImageResourcePlacement) {
    match placement {
        ImageResourcePlacement::Atlas(rect) => patch_atlas_resource_pattern(blob, base, rect),
        ImageResourcePlacement::Texture(rect) => {
            if rect.width > 0 && rect.height > 0 {
                blob[base + 2] = 0;
                blob[base + 3] = 0;
                // Word 4 stores either an atlas page or a tagged texture-table index.
                // The high bit selects texture placement; the remaining bits store the index.
                blob[base + 4] = GPU_RESOURCE_TEXTURE_PLACEMENT_BIT | rect.index;
                blob[base + 5] = rect.width;
                blob[base + 6] = rect.height;
            } else {
                clear_missing_resource_pattern(blob, base);
            }
        }
    }
}

fn patch_atlas_resource_pattern(blob: &mut [u32], base: usize, rect: AtlasRect) {
    if rect.width > 0 && rect.height > 0 {
        blob[base + 2] = rect.x;
        blob[base + 3] = rect.y;
        blob[base + 4] = rect.page;
        blob[base + 5] = rect.width;
        blob[base + 6] = rect.height;
    } else {
        clear_missing_resource_pattern(blob, base);
    }
}

fn clear_missing_resource_pattern(blob: &mut [u32], base: usize) {
    blob[base] = GPU_BRUSH_PATTERN;
    blob[base + 2] = ENCODED_BRUSH_HEADER_WORDS as u32;
    blob[base + 3] = 0;
    blob[base + 4] = 0;
    blob[base + 5] = 0;
    blob[base + 6] = 0;
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
mod tests {
    use peniko::{Color, Extend};

    use super::*;
    use crate::shared::{
        bounds::PixelBounds,
        brush::{PatternBrush, PatternSampling},
        draw_record::{DrawTagWord, FillRuleWord},
        fill::FillRule,
        image_resource::ImageKey,
    };

    fn draw_with_brush(brush_offset: u32, brush_len: u32) -> DrawRecord {
        DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: DrawTagWord(0),
            fill_rule: FillRuleWord(FillRule::NonZero as u32),
            pixel_bounds: PixelBounds::default(),
            solid_rect: 0,
        }
    }

    #[test]
    fn scene_brushes_need_resource_patch_only_for_resource_patterns() {
        let mut blob = Vec::new();
        let solid = Brush::Solid(Color::from_rgb8(16, 32, 48));
        let (solid_offset, solid_len) = push_encoded_brush(&mut blob, &solid);
        assert!(!GpuBrushUpload::scene_brushes_need_resource_patch(
            &[draw_with_brush(solid_offset, solid_len)],
            &blob
        ));

        let key = ImageKey::new(0x1234_5678);
        let resource = Brush::Pattern(
            PatternBrush::new_resource(
                ImageResourceId::renderer(key),
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                Extend::Pad,
                PatternSampling::Nearest,
                255,
            )
            .unwrap(),
        );
        let (resource_offset, resource_len) = push_encoded_brush(&mut blob, &resource);
        assert!(GpuBrushUpload::scene_brushes_need_resource_patch(
            &[draw_with_brush(resource_offset, resource_len)],
            &blob
        ));
    }

    #[test]
    fn patch_scene_brush_blob_clears_missing_resource_placement() {
        let key = ImageKey::new(0x1234_5678);
        let resource = Brush::Pattern(
            PatternBrush::new_resource(
                ImageResourceId::renderer(key),
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                Extend::Pad,
                PatternSampling::Nearest,
                255,
            )
            .unwrap(),
        );
        let mut blob = Vec::new();
        let (offset, len) = push_encoded_brush(&mut blob, &resource);
        let draw = draw_with_brush(offset, len);
        assert_eq!(blob[offset as usize], GPU_BRUSH_PATTERN_RESOURCE);

        GpuBrushUpload::patch_scene_brush_blob(&mut blob, &[draw], None);

        let base = offset as usize;
        assert_eq!(blob[base], GPU_BRUSH_PATTERN);
        assert_eq!(blob[base + 2], ENCODED_BRUSH_HEADER_WORDS as u32);
        assert_eq!(blob[base + 3], 0);
        assert_eq!(blob[base + 4], 0);
        assert_eq!(blob[base + 5], 0);
        assert_eq!(blob[base + 6], 0);
    }
}
