//! Native filter brushes retain the exact CPU record starts and image placement
//! association. Raw shader offsets cannot select a header's interior or padding.
use super::region;
use crate::native::runtime::Result;
use crate::native::runtime::{
    compute::{ComputeBatch, ResourceId},
    program::scene::SceneImages,
};
use crate::shared::{filter_config::FilterConfig, gpu_brush::GpuBrushUpload};

pub(crate) struct Brushes {
    buffer: ResourceId,
    offsets: Vec<u32>,
}
impl Brushes {
    pub(crate) fn record(
        batch: &mut ComputeBatch,
        ops: &[crate::shared::execution::ExecOp],
        filter: Option<&crate::shared::layer::filter::Filter>,
        images: &crate::shared::image_resource::GpuImageResourceUpload,
    ) -> Result<Option<Self>> {
        let upload = if let Some(filter) = filter {
            GpuBrushUpload::from_filter_ops_and_filter_with_resources(ops, filter, Some(images))
        } else {
            GpuBrushUpload::from_filter_plan_with_resources(ops, Some(images))
        };

        if upload.offsets.is_empty() {
            return Ok(None);
        }
        if upload.blob.len() > u32::MAX as usize / 4 {
            return Err("filter brush byte addressing overflow".into());
        }
        Ok(Some(Self {
            buffer: batch.buffer(bytemuck::cast_slice(&upload.blob).to_vec())?,
            offsets: upload.offsets,
        }))
    }
    /// # Safety
    /// Images must be the immutable upload pair that patched these brush records.
    pub(crate) unsafe fn encode(
        &self,
        batch: &mut ComputeBatch,
        config: FilterConfig,
        tiles: Option<&[u32]>,
        images: &SceneImages,
        shadow: Option<ResourceId>,
        target: ResourceId,
    ) -> Result<()> {
        if self.offsets.binary_search(&config.brush_offset).is_err() {
            return Err("filter brush offset is not a prepared record start".into());
        }
        let reads = shadow.map(|id| (2, id));
        region::record(
            batch,
            if shadow.is_some() {
                "filter_composite_drop_shadow_region"
            } else {
                "filter_flood_region"
            },
            region::Geometry::Pixels,
            config,
            tiles,
            region::ReadBindings {
                textures: reads.as_slice(),
                texture_extent: [config.width, config.height],
                buffers: &[(10, self.buffer)],
                images: Some(images),
            },
            target,
        )
    }
}

#[cfg(test)]
#[path = "../../tests/filter_brush.rs"]
mod tests;
