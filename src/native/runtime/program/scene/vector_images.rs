//! Render child scenes into the same batch, then populate their shared placements.
use super::SceneImages;
use crate::{
    Canvas,
    native::runtime::{
        Result,
        compute::{ComputeBatch, Resource, ResourceId, TextureCopy},
    },
    render::vector_images::atlas_copy_regions,
    shared::image_resource::{GpuImageResourceUpload, ImageResourcePlacement},
};
use std::rc::Rc;

impl SceneImages {
    /// The frame's renderer supplies each child output in this batch. Fresh native
    /// allocations need every vector even when the CPU placement cache is clean.
    /// On any error the frame owner must discard this unsubmitted batch.
    pub(crate) fn record_with_vectors(
        batch: &mut ComputeBatch,
        upload: &GpuImageResourceUpload,
        mut render: impl FnMut(&mut ComputeBatch, &Rc<Canvas>) -> Result<ResourceId>,
    ) -> Result<Self> {
        let mut images = Self::allocate(batch, upload)?;
        for vector in upload.vectors() {
            let source = render(batch, &vector.canvas)?;
            batch.size(source)?;
            let size = vector.canvas.physical_size();
            if !matches!(&batch.resources()[source.index()], Resource::Texture(texture)
                if !texture.array && texture.size == [size.0, size.1]
                    && texture.format == crate::ShaderTextureFormat::Rgba8Unorm)
            {
                return Err(
                    "native vector output must be RGBA8 and match its Canvas dimensions".into(),
                );
            }
            match vector.placement {
                ImageResourcePlacement::Texture(rect) => {
                    // Bind the child's complete image directly. Cloning the native
                    // lease later pins it for the immutable renderer image cache.
                    images.textures[rect.index as usize] = source;
                }
                ImageResourcePlacement::Atlas(rect) => {
                    for region in atlas_copy_regions(rect) {
                        batch.copy_texture(TextureCopy {
                            source,
                            destination: images.atlas,
                            source_origin: [region.source[0], region.source[1], 0],
                            destination_origin: region.destination,
                            extent: [region.extent[0], region.extent[1], 1],
                        })?;
                    }
                }
            }
        }
        images.finish(batch)
    }
}
