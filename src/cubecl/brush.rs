use ::cubecl::prelude::Runtime;
use peniko::Extend;

use crate::{
    scene::Scene,
    shared::{
        brush::{Brush, PatternSampling},
        execution::ExecOp,
        image::premul_color_to_rgba8_pack,
        layer::{
            Layer,
            filter::{Filter, FilterPrimitiveKind},
        },
    },
};

#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

use super::buffer::CubeBuffer;

pub(crate) const GPU_BRUSH_U32_STRIDE: usize = 9;
pub(crate) const GPU_BRUSH_PARAM_STRIDE: usize = 12;
pub(crate) const GPU_BRUSH_SOLID: u32 = 1;
pub(crate) const GPU_BRUSH_LINEAR: u32 = 2;
pub(crate) const GPU_BRUSH_RADIAL: u32 = 3;
pub(crate) const GPU_BRUSH_SWEEP: u32 = 4;
pub(crate) const GPU_BRUSH_FOUR_CORNER: u32 = 5;
pub(crate) const GPU_BRUSH_PATTERN: u32 = 6;

pub(crate) const GPU_PATTERN_NEAREST: u32 = 0;
pub(crate) const GPU_PATTERN_BILINEAR: u32 = 1;

pub(crate) const GPU_EXTEND_PAD: u32 = 0;
pub(crate) const GPU_EXTEND_REPEAT: u32 = 1;
pub(crate) const GPU_EXTEND_REFLECT: u32 = 2;

pub(crate) struct GpuBrushResources<'a> {
    pub(crate) data: &'a CubeBuffer<u32>,
    pub(crate) params: &'a CubeBuffer<f32>,
    pub(crate) payloads: &'a CubeBuffer<u32>,
}

pub(crate) struct GpuBrushBuffers {
    pub(crate) data: CubeBuffer<u32>,
    pub(crate) params: CubeBuffer<f32>,
    pub(crate) payloads: CubeBuffer<u32>,
}

impl GpuBrushBuffers {
    pub(crate) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            data: CubeBuffer::new(client, 0),
            params: CubeBuffer::new(client, 0),
            payloads: CubeBuffer::new(client, 0),
        }
    }

    pub(crate) fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        upload: GpuBrushUpload,
    ) {
        self.data.replace(client, &upload.data);
        self.params.replace(client, &upload.params);
        self.payloads.replace(client, &upload.payloads);
    }

    pub(crate) fn resources(&self) -> GpuBrushResources<'_> {
        GpuBrushResources {
            data: &self.data,
            params: &self.params,
            payloads: &self.payloads,
        }
    }

    #[cfg(feature = "profile")]
    pub(crate) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            self.data.memory_usage(),
            self.params.memory_usage(),
            self.payloads.memory_usage(),
        ])
    }
}

#[derive(Default)]
pub(crate) struct GpuBrushUpload {
    data: Vec<u32>,
    params: Vec<f32>,
    payloads: Vec<u32>,
}

impl GpuBrushUpload {
    pub(crate) fn from_scene_draws(scene: &Scene) -> Self {
        let mut upload = Self::default();
        for draw in &scene.draw_records {
            upload.push_brush(&draw.brush);
        }
        upload
    }

    pub(crate) fn from_filter_plan(ops: &[ExecOp]) -> Self {
        let mut upload = Self::default();
        collect_filter_brushes_for_ops(ops, &mut upload);
        upload
    }

    pub(crate) fn from_filter_ops_and_filter(ops: &[ExecOp], filter: &Filter) -> Self {
        let mut upload = Self::default();
        collect_filter_brushes_for_ops(ops, &mut upload);
        collect_filter_brush(filter, &mut upload);
        upload
    }

    fn push_brush(&mut self, brush: &Brush) {
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
                kind = GPU_BRUSH_PATTERN;
                extend = encode_gpu_extend(pattern.extend);
                params[0..6].copy_from_slice(&pattern.transform);
                image_width = pattern.image.width;
                image_height = pattern.image.height;
                opacity = pattern.opacity as u32;
                pattern_sampling = encode_gpu_pattern_sampling(pattern.sampling);
                (payload_offset, payload_len) = self.push_payload(&pattern.image.pixels);
            }
        }

        self.data.extend_from_slice(&[
            kind,
            extend,
            payload_offset,
            payload_len,
            color,
            image_width,
            image_height,
            opacity,
            pattern_sampling,
        ]);
        debug_assert_eq!(self.data.len() % GPU_BRUSH_U32_STRIDE, 0);
        self.params.extend_from_slice(&params);
    }

    fn push_payload(&mut self, payload: &[u32]) -> (u32, u32) {
        let offset = self.payloads.len() as u32;
        self.payloads.extend_from_slice(payload);
        (offset, payload.len() as u32)
    }
}

fn collect_filter_brushes_for_ops(ops: &[ExecOp], upload: &mut GpuBrushUpload) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                layer, children, ..
            } => match layer {
                Layer::Filter { filter, .. } => {
                    collect_filter_brushes_for_ops(children, upload);
                    collect_filter_brush(filter, upload);
                }
                Layer::Backdrop { filter, .. } => {
                    collect_filter_brush(filter, upload);
                    collect_filter_brushes_for_ops(children, upload);
                }
                _ => collect_filter_brushes_for_ops(children, upload),
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                collect_filter_brushes_for_ops(content, upload);
                collect_filter_brushes_for_ops(mask, upload);
            }
            _ => {}
        }
    }
}

fn collect_filter_brush(filter: &Filter, upload: &mut GpuBrushUpload) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                collect_filter_brush(filter, upload);
            }
        }
        Filter::Graph { primitives, .. } => {
            for primitive in primitives {
                match &primitive.kind {
                    FilterPrimitiveKind::Filter(filter) => collect_filter_brush(filter, upload),
                    FilterPrimitiveKind::Image { brush } => upload.push_brush(brush),
                    _ => {}
                }
            }
        }
        Filter::DropShadow { brush, .. } => upload.push_brush(brush),
        Filter::Flood { brush } => upload.push_brush(brush),
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
