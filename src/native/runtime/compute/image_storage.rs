#[cfg(feature = "metal")]
use super::TextureCopy;
use super::{ComputeBatch, Resource, ResourceId};
use crate::native::{NativeTexture, runtime::Result};

impl ComputeBatch {
    /// Transfer a fully initialized image to the renderer's immutable image cache.
    /// Its source stage may write before this call; later commands may only read it.
    /// Windows uploads directly into a pooled lease, removing the duplicate atlas
    /// allocation/copy at its source. Metal retains its existing copy implementation.
    pub(crate) fn retain_image(&mut self, source: ResourceId) -> Result<NativeTexture> {
        self.size(source)?;
        let Resource::Texture(input) = &self.resources()[source.index()] else {
            return Err("native image snapshot requires a texture".into());
        };
        let (size, layers, array) = (input.size, input.layers, input.array);
        #[cfg(any(feature = "dx12", feature = "vulkan"))]
        {
            if input.persistent.is_some() || input.bytes.is_empty() {
                return Err("image retention requires initialized batch-owned storage".into());
            }
            let texture = self
                .surface_pool
                .as_ref()
                .ok_or("image retention needs a renderer context")?
                .borrow_mut()
                .acquire_image(size, layers, array)?;
            let Resource::Texture(input) = &mut self.resources[source.index()] else {
                unreachable!("validated image resource")
            };
            input.persistent = Some(texture.clone());
            Ok(texture)
        }
        #[cfg(feature = "metal")]
        {
            let texture = self
                .context()
                .ok_or("native snapshot needs a renderer context")?
                .create_texture_kind(size, layers, array)?;
            // This private import never exposes an undefined array for sampling: its
            // next command covers every layer. Acceptance alone publishes initialization.
            let destination = self.import_texture_inner(&texture, false)?;
            self.copy_texture(TextureCopy {
                source,
                destination,
                source_origin: [0; 3],
                destination_origin: [0; 3],
                extent: [size[0], size[1], layers],
            })?;
            Ok(texture)
        }
    }
}
