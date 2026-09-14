use super::{
    region,
    resample::{self, Resample},
};
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::filter_config::FilterConfig,
};
#[derive(Clone, Copy, Debug)]
pub enum RectanglePass {
    Mask,
    Direct {
        source: ResourceId,
        mask: Option<ResourceId>,
    },
    Rectangle {
        source: ResourceId,
    },
    UpsampleRectangle {
        source: ResourceId,
    },
}
pub fn encode(
    batch: &mut ComputeBatch,
    pass: RectanglePass,
    mut config: FilterConfig,
    tiles: Option<&[u32]>,
    target: ResourceId,
) -> Result<()> {
    if !matches!(pass, RectanglePass::Direct { .. }) {
        let values = [
            config.rect_x0,
            config.rect_y0,
            config.rect_x1,
            config.rect_y1,
            config.radius_top_left,
            config.radius_top_right,
            config.radius_bottom_left,
            config.radius_bottom_right,
        ];
        if values
            .iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX / 4.0)
        {
            return Err("invalid rectangle SDF parameters".into());
        }
    }
    if matches!(pass, RectanglePass::UpsampleRectangle { .. }) {
        resample::validate_sampling(config, Resample::Upsample)?;
        if config.source_x0 > config.source_x1
            || config.source_y0 > config.source_y1
            || config.source_x1 > config.width
            || config.source_y1 > config.height
        {
            return Err("invalid upsample rectangle source bounds".into());
        }
    }
    let (entry, source, mask) = match pass {
        RectanglePass::Mask => ("filter_rect_mask_region", None, None),
        RectanglePass::Direct { source, mask } => {
            // Derive the switch from the resource; the unused static binding aliases source.
            config.mask_enabled = u32::from(mask.is_some());
            (
                "filter_composite_direct_region",
                Some(source),
                Some(mask.unwrap_or(source)),
            )
        }
        RectanglePass::Rectangle { source } => {
            ("filter_composite_rect_direct_region", Some(source), None)
        }
        RectanglePass::UpsampleRectangle { source } => {
            ("filter_upsample_rect_composite_region", Some(source), None)
        }
    };
    let reads: Vec<_> = source
        .map(|id| (1, id))
        .into_iter()
        .chain(mask.map(|id| (2, id)))
        .collect();
    region::record(
        batch,
        entry,
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&reads, [config.width, config.height]),
        target,
    )
}
