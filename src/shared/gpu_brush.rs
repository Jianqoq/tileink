use peniko::Extend;

pub(crate) use crate::shared::gpu_layout::brush::{
    GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_PARAM_STRIDE, GPU_BRUSH_PATTERN,
    GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_RADIAL, GPU_BRUSH_SOLID, GPU_BRUSH_SWEEP,
    GPU_BRUSH_U32_STRIDE, GPU_EXTEND_PAD, GPU_EXTEND_REFLECT, GPU_EXTEND_REPEAT,
    GPU_PATTERN_BILINEAR, GPU_PATTERN_NEAREST,
};

use crate::shared::{
    brush::{Brush, PatternSampling},
    draw_record::DrawRecord,
    execution::ExecOp,
    image::premul_color_to_rgba8_pack,
    image_resource::GpuImageResourceUpload,
    layer::{
        Layer,
        filter::{Filter, FilterPrimitiveKind},
    },
};

#[derive(Clone, Default)]
pub(crate) struct GpuBrushUpload {
    pub(crate) data: Vec<u32>,
    pub(crate) params: Vec<f32>,
    pub(crate) payloads: Vec<u32>,
}

impl GpuBrushUpload {
    pub(crate) fn clear(&mut self) {
        self.data.clear();
        self.params.clear();
        self.payloads.clear();
    }

    pub(crate) fn from_scene_draws_raw(draws: &[DrawRecord]) -> Self {
        Self::from_scene_draws(draws, None)
    }

    pub(crate) fn from_scene_draws(
        draws: &[DrawRecord],
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut upload = Self::default();
        for draw in draws {
            upload.push_brush_with_resources(&draw.brush, image_resources);
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

    pub(crate) fn push_brush(&mut self, brush: &Brush) {
        self.push_brush_with_resources(brush, None);
    }

    pub(crate) fn push_brush_with_resources(
        &mut self,
        brush: &Brush,
        image_resources: Option<&GpuImageResourceUpload>,
    ) {
        let (data, params) = self.encode_brush(brush, image_resources);
        self.data.extend_from_slice(&data);
        debug_assert_eq!(self.data.len() % GPU_BRUSH_U32_STRIDE, 0);
        self.params.extend_from_slice(&params);
    }

    pub(crate) fn write_solid_color(&mut self, index: usize, color: peniko::Color) {
        let data_offset = index * GPU_BRUSH_U32_STRIDE;
        let params_offset = index * GPU_BRUSH_PARAM_STRIDE;
        assert!(
            data_offset + GPU_BRUSH_U32_STRIDE <= self.data.len()
                && params_offset + GPU_BRUSH_PARAM_STRIDE <= self.params.len(),
            "brush index out of range"
        );
        let brush = Brush::Solid(color);
        let (data, params) = self.encode_brush(&brush, None);
        self.data[data_offset..data_offset + GPU_BRUSH_U32_STRIDE].copy_from_slice(&data);
        self.params[params_offset..params_offset + GPU_BRUSH_PARAM_STRIDE].copy_from_slice(&params);
    }

    fn encode_brush(
        &mut self,
        brush: &Brush,
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> ([u32; GPU_BRUSH_U32_STRIDE], [f32; GPU_BRUSH_PARAM_STRIDE]) {
        let mut params = [0.0; GPU_BRUSH_PARAM_STRIDE];
        let mut kind = GPU_BRUSH_SOLID;
        let mut extend = GPU_EXTEND_PAD;
        let mut color = 0;
        let mut image_width = 0;
        let mut image_height = 0;
        let mut opacity = 255;
        let mut pattern_sampling = GPU_PATTERN_NEAREST;
        let mut payload_offset = 0;
        let mut payload_len = 0;

        match brush {
            Brush::Solid(value) => {
                color = premul_color_to_rgba8_pack(*value);
            }
            Brush::Linear(gradient) => {
                kind = GPU_BRUSH_LINEAR;
                extend = encode_gpu_extend(gradient.extend);
                params[0] = gradient.start[0];
                params[1] = gradient.start[1];
                params[2] = gradient.end[0];
                params[3] = gradient.end[1];
                params[4..10].copy_from_slice(&gradient.transform);
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::Radial(gradient) => {
                kind = GPU_BRUSH_RADIAL;
                extend = encode_gpu_extend(gradient.extend);
                params[0] = gradient.start_center[0];
                params[1] = gradient.start_center[1];
                params[2] = gradient.end_center[0];
                params[3] = gradient.end_center[1];
                params[4] = gradient.start_radius;
                params[5] = gradient.end_radius;
                params[6..12].copy_from_slice(&gradient.transform);
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::Sweep(gradient) => {
                kind = GPU_BRUSH_SWEEP;
                extend = encode_gpu_extend(gradient.extend);
                params[0] = gradient.center[0];
                params[1] = gradient.center[1];
                params[2] = gradient.start_angle;
                params[3] = gradient.end_angle;
                (payload_offset, payload_len) = self.push_payload(&gradient.ramp);
            }
            Brush::FourCorner(gradient) => {
                kind = GPU_BRUSH_FOUR_CORNER;
                params[0..4].copy_from_slice(&gradient.bounds);
                (payload_offset, payload_len) = self.push_payload(&gradient.colors);
            }
            Brush::Pattern(pattern) => {
                extend = encode_gpu_extend(pattern.extend);
                params[0..6].copy_from_slice(&pattern.transform);
                let (width, height) = pattern.image_size();
                image_width = width;
                image_height = height;
                opacity = pattern.opacity as u32;
                pattern_sampling = encode_gpu_pattern_sampling(pattern.sampling);
                if let Some(key) = pattern.image_key() {
                    if let Some(index) =
                        image_resources.and_then(|resources| resources.image_index(key))
                    {
                        kind = GPU_BRUSH_PATTERN_RESOURCE;
                        payload_offset = index;
                    } else {
                        kind = GPU_BRUSH_PATTERN;
                    }
                } else if let Some(image) = pattern.inline_image() {
                    kind = GPU_BRUSH_PATTERN;
                    (payload_offset, payload_len) = self.push_payload(&image.pixels);
                }
            }
        }

        (
            [
                kind,
                extend,
                payload_offset,
                payload_len,
                color,
                image_width,
                image_height,
                opacity,
                pattern_sampling,
            ],
            params,
        )
    }

    fn push_payload(&mut self, payload: &[u32]) -> (u32, u32) {
        let offset = self.payloads.len() as u32;
        self.payloads.extend_from_slice(payload);
        (offset, payload.len() as u32)
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
                        upload.push_brush_with_resources(brush, image_resources)
                    }
                    _ => {}
                }
            }
        }
        Filter::DropShadow { brush, .. } => {
            upload.push_brush_with_resources(brush, image_resources)
        }
        Filter::Flood { brush } => upload.push_brush_with_resources(brush, image_resources),
        _ => {}
    }
}

fn encode_gpu_extend(extend: Extend) -> u32 {
    match extend {
        Extend::Pad => GPU_EXTEND_PAD,
        Extend::Repeat => GPU_EXTEND_REPEAT,
        Extend::Reflect => GPU_EXTEND_REFLECT,
    }
}

fn encode_gpu_pattern_sampling(sampling: PatternSampling) -> u32 {
    match sampling {
        PatternSampling::Nearest => GPU_PATTERN_NEAREST,
        PatternSampling::Bilinear => GPU_PATTERN_BILINEAR,
    }
}
