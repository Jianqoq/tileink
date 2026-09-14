use super::Reference;
use crate::native::runtime::{Result, compute::ComputeBatch};

#[derive(Clone, Copy, Debug)]
pub struct FineVariant {
    pub portable: bool,
    pub texture_table: bool,
}
impl FineVariant {
    pub const ALL: [Self; 4] = [
        Self {
            portable: false,
            texture_table: false,
        },
        Self {
            portable: false,
            texture_table: true,
        },
        Self {
            portable: true,
            texture_table: false,
        },
        Self {
            portable: true,
            texture_table: true,
        },
    ];
    pub(super) fn source(self) -> String {
        let source = if self.portable {
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl"))
        } else {
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine.wgsl"))
        };
        // Only binding locations change. Both production target modes and image
        // variants retain their actual shader implementations.
        crate::wgpu::shader_variants::patch_image_resource_shader_source(source, self.texture_table)
            .replace("@group(1) @binding(0)", "@group(0) @binding(12)")
            .replace("@group(1) @binding(1)", "@group(0) @binding(13)")
            .replace("@group(1) @binding(2)", "@group(1) @binding(30)")
    }
}
impl Reference {
    pub fn execute_fine_variant(
        &self,
        batch: &ComputeBatch,
        variant: FineVariant,
    ) -> Result<Vec<Vec<u8>>> {
        self.execute_selected(batch, None, Some(variant))
    }
}
