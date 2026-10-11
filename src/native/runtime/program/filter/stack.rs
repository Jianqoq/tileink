use super::{layer::Geometry, region, surface};
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::filter_config::FilterConfig,
};

#[derive(Clone, Copy)]
pub enum Composite {
    Over,
    Blend,
    Surface,
}

pub struct Textures {
    pub source: ResourceId,
    pub auxiliary: Option<ResourceId>,
    pub target: ResourceId,
}

/// Validated layer references, independent of source surfaces and per-pass regions.
pub struct Stack {
    geometry: Geometry,
    layers: ResourceId,
    count: u32,
}
impl Stack {
    /// # Safety
    /// `layers` must contain `count` valid records for this geometry, with byte
    /// opacity payloads and bounded draw indices, in the geometry's batch.
    pub(in crate::native::runtime::program) unsafe fn from_gpu(
        geometry: Geometry,
        layers: ResourceId,
        count: u32,
    ) -> Self {
        Self {
            geometry,
            layers,
            count,
        }
    }

    pub fn encode(
        &self,
        batch: &mut ComputeBatch,
        mode: Composite,
        mut config: FilterConfig,
        tiles: Option<&[u32]>,
        textures: Textures,
    ) -> Result<()> {
        if config.layer_stack_start > config.layer_stack_end || config.layer_stack_end > self.count
        {
            return Err("composite layer range exceeds logical stack".into());
        }
        if config.width > i32::MAX as u32 || config.height > i32::MAX as u32 {
            return Err("composite layer coordinates exceed signed range".into());
        }
        config.paint_sdf_shadow_base = self.geometry.shadow_base();
        let (entry, extent, reads) = match mode {
            Composite::Surface => {
                config.mask_enabled = 0;
                surface::validate(&config)?;
                (
                    "filter_composite_surface_stack_region",
                    [config.kernel_columns, config.kernel_rows],
                    vec![(1, textures.source)],
                )
            }
            Composite::Over | Composite::Blend => {
                let auxiliary = if matches!(mode, Composite::Blend) {
                    config.mask_enabled = 1;
                    textures
                        .auxiliary
                        .ok_or("stack blend auxiliary texture is required")?
                } else {
                    config.mask_enabled = u32::from(textures.auxiliary.is_some());
                    // The pipeline still requires t2 when masking is disabled. Reuse the
                    // read-only source descriptor instead of allocating a dummy texture.
                    textures.auxiliary.unwrap_or(textures.source)
                };
                (
                    if matches!(mode, Composite::Blend) {
                        "filter_composite_blend_stack_region"
                    } else {
                        "filter_composite_stack_region"
                    },
                    [config.width, config.height],
                    vec![(1, textures.source), (2, auxiliary)],
                )
            }
        };
        let mut buffers = self.geometry.bindings().to_vec();
        buffers.push((25, self.layers));
        region::record(
            batch,
            entry,
            region::Geometry::Pixels,
            config,
            tiles,
            region::ReadBindings {
                images: None,
                textures: &reads,
                texture_extent: extent,
                buffers: &buffers,
            },
            textures.target,
        )
    }
}
