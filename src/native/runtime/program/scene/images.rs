//! Materialize the shared placement upload without changing its texel encoding.
use super::SceneImages;
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, ResourceId, SamplerFilter},
};
use crate::shared::{
    gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
    image_resource::{GpuImageResourceUpload, ImageResourcePlacement},
};

/// Build the binding table only after vector children have supplied their outputs.
/// Unused slots still share the required initialized one-pixel transparent texture.
pub(super) struct ImageAllocation {
    pub(super) atlas: ResourceId,
    pub(super) textures: Vec<ResourceId>,
}

impl ImageAllocation {
    pub(super) fn finish(self, batch: &mut ComputeBatch) -> Result<SceneImages> {
        Ok(SceneImages {
            atlas: self.atlas,
            table: batch.texture_table(&self.textures)?,
            sampler: batch.sampler(SamplerFilter::Linear),
        })
    }
}

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
        Self::allocate(batch, upload)?.finish(batch)
    }

    pub(super) fn allocate(
        batch: &mut ComputeBatch,
        upload: &GpuImageResourceUpload,
    ) -> Result<ImageAllocation> {
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
            let pixels = if pages.len() == 1 {
                std::rc::Rc::clone(&pages[0].pixels)
            } else {
                let mut pixels = Vec::new();
                pixels.try_reserve_exact(bytes / 4)?;
                for page in pages {
                    pixels.extend_from_slice(&page.pixels);
                }
                std::rc::Rc::new(pixels)
            };
            batch.texture_pixels([size, size, u32::try_from(pages.len())?], true, pixels)?
        };
        let empty = batch.texture_rgba8([1, 1], vec![0; 4])?;
        let mut textures = vec![empty; NATIVE_TEXTURE_TABLE_CAPACITY as usize];
        for texture in upload.textures() {
            let slot = textures
                .get_mut(texture.index as usize)
                .ok_or("native image texture index exceeds shader table")?;
            if texture.pixels.is_empty()
                && upload.vectors().iter().any(|vector| {
                    matches!(vector.placement,
                        ImageResourcePlacement::Texture(rect) if rect.index == texture.index
                    )
                })
            {
                // Validate before invoking the child renderer. No placeholder pixel
                // allocation is needed: the completed child owns this entire table slot.
                if [texture.width, texture.height]
                    .into_iter()
                    .any(|n| n == 0 || n > i32::MAX as u32)
                {
                    return Err("native vector texture dimensions exceed texture limits".into());
                }
            } else {
                *slot = batch.texture_pixels(
                    [texture.width, texture.height, 1],
                    false,
                    texture.pixels.clone(),
                )?;
            }
        }
        Ok(ImageAllocation { atlas, textures })
    }
}

#[cfg(test)]
#[path = "../../tests/scene_images.rs"]
mod tests;
