use ::cubecl::prelude::Runtime;

#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

use super::buffer::CubeBuffer;

#[cfg(test)]
pub(crate) use crate::shared::gpu_brush::GPU_BRUSH_SOLID;
pub(crate) use crate::shared::gpu_brush::{
    GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_PARAM_STRIDE, GPU_BRUSH_PATTERN,
    GPU_BRUSH_RADIAL, GPU_BRUSH_SWEEP, GPU_BRUSH_U32_STRIDE, GPU_EXTEND_REFLECT, GPU_EXTEND_REPEAT,
    GPU_PATTERN_BILINEAR, GpuBrushUpload,
};

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
        upload: &GpuBrushUpload,
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
