//! Ordered texture transfers share submission ownership with compute dispatches.
use super::{ComputeBatch, Resource, ResourceId};
use crate::native::runtime::Result;

#[derive(Clone, Copy, Debug)]
pub struct TextureCopy {
    pub source: ResourceId,
    pub destination: ResourceId,
    /// x, y, array layer; every texture has one mip level.
    pub source_origin: [u32; 3],
    pub destination_origin: [u32; 3],
    pub extent: [u32; 3],
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Dispatch(usize),
    CopyTexture(TextureCopy),
}

impl ComputeBatch {
    /// Copies raw RGBA8 texels, without sampling or color conversion. Distinct
    /// resources are required; self-copy would have incompatible API states.
    pub fn copy_texture(&mut self, copy: TextureCopy) -> Result<()> {
        if copy.source == copy.destination {
            return Err("native texture copy requires distinct resources".into());
        }
        for (id, origin) in [
            (copy.source, copy.source_origin),
            (copy.destination, copy.destination_origin),
        ] {
            self.size(id)?;
            let Resource::Texture(texture) = &self.resources[id.index()] else {
                return Err("native texture copy requires image resources".into());
            };
            for ((offset, count), capacity) in origin.into_iter().zip(copy.extent).zip([
                texture.size[0],
                texture.size[1],
                texture.layers,
            ]) {
                if offset.checked_add(count).is_none_or(|end| end > capacity) {
                    return Err("native texture copy region exceeds image".into());
                }
            }
        }
        if !copy.extent.contains(&0) {
            self.commands.push(Command::CopyTexture(copy));
        }
        Ok(())
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
}

#[cfg(test)]
#[path = "../tests/texture_copy.rs"]
mod tests;
