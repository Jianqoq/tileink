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
    programs: Vec<u32>,
    texture_start: usize,
}
impl Brushes {
    pub(crate) fn record(
        batch: &mut ComputeBatch,
        upload: GpuBrushUpload,
        texture_start: usize,
    ) -> Result<Option<Self>> {
        if upload.offsets.is_empty() {
            return Ok(None);
        }
        if upload.blob.len() > u32::MAX as usize / 4 {
            return Err("filter brush byte addressing overflow".into());
        }
        let buffer = batch.buffer(bytemuck::cast_slice(&upload.blob).to_vec())?;
        Ok(Some(Self {
            programs: upload.blob,
            texture_start,
            buffer,
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
        let (buffer, owned_images) = crate::native::runtime::program_shader::brushes_for_filter(
            batch,
            &self.programs,
            &self.offsets,
            self.texture_start,
            config,
            images,
        )?;
        let images = &owned_images;
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
                buffers: &[(10, buffer.unwrap_or(self.buffer))],
                images: Some(images),
            },
            target,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/filter_brush.rs"]
mod tests;
