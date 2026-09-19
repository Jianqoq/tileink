use super::{ComputeBatch, Resource, ResourceId, TextureCopy};
use crate::native::{NativeTexture, runtime::Result};

impl ComputeBatch {
    /// Copy a fully defined batch texture into persistent storage for later frames.
    /// The source stage owns initialization of every pixel and array layer.
    pub(crate) fn snapshot_texture(&mut self, source: ResourceId) -> Result<NativeTexture> {
        self.size(source)?;
        let Resource::Texture(input) = &self.resources()[source.index()] else {
            return Err("native image snapshot requires a texture".into());
        };
        let (size, layers, array) = (input.size, input.layers, input.array);
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
