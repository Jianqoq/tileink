//! Materialize the shared placement upload without changing its texel encoding.
use super::SceneImages;
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, SamplerFilter},
};
use crate::shared::{
    gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY, image_resource::GpuImageResourceUpload,
};

impl SceneImages {
    /// Vector placements must first be rendered by frame assembly. Reject them
    /// here instead of silently sampling the zero-filled raster placeholders.
    pub(crate) fn record(
        batch: &mut ComputeBatch,
        upload: &GpuImageResourceUpload,
    ) -> Result<Self> {
        if !upload.vectors().is_empty() {
            return Err("native vector images require frame rendering before sampling".into());
        }
        let pages = upload.atlas_pages();
        let atlas = if pages.is_empty() {
            batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?
        } else {
            let size = upload.atlas_page_size();
            if size == 0 || size > i32::MAX as u32 || pages.len() > u16::MAX as usize {
                return Err("native image atlas dimensions exceed texture limits".into());
            }
            let page_bytes = (size as usize)
                .checked_mul(size as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or("native image atlas page size overflow")?;
            let bytes = page_bytes
                .checked_mul(pages.len())
                .ok_or("native image atlas size overflow")?;
            for (index, page) in pages.iter().enumerate() {
                if page.index as usize != index
                    || page.size != size
                    || page.pixels.len().checked_mul(4) != Some(page_bytes)
                {
                    return Err("native image atlas page layout mismatch".into());
                }
            }
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(bytes)?;
            for page in pages {
                pixels.extend_from_slice(bytemuck::cast_slice(&page.pixels));
            }
            batch.texture_array_rgba8([size, size, u32::try_from(pages.len())?], pixels)?
        };
        let empty = batch.texture_rgba8([1, 1], vec![0; 4])?;
        let mut textures = vec![empty; NATIVE_TEXTURE_TABLE_CAPACITY as usize];
        for texture in upload.textures() {
            let slot = textures
                .get_mut(texture.index as usize)
                .ok_or("native image texture index exceeds shader table")?;
            *slot = batch.texture_rgba8(
                [texture.width, texture.height],
                bytemuck::cast_slice(&texture.pixels).to_vec(),
            )?;
        }
        Ok(Self {
            atlas,
            table: batch.texture_table(&textures)?,
            sampler: batch.sampler(SamplerFilter::Linear)?,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/scene_images.rs"]
mod tests;
