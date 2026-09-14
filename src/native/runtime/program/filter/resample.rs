use super::region;
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::filter_config::FilterConfig,
};
#[derive(Clone, Copy, Debug)]
pub enum Resample {
    Downsample,
    Upsample,
}

pub fn encode(
    batch: &mut ComputeBatch,
    stage: Resample,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    let rectangle = [
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
    ];
    if rectangle.iter().any(|v| !v.is_finite() || *v < 0.0)
        || config.rect_x0 > config.rect_x1
        || config.rect_y0 > config.rect_y1
        || f64::from(config.rect_x1) > f64::from(config.width)
        || f64::from(config.rect_y1) > f64::from(config.height)
        || config.downsample_filter > 1
        || config.upsample_filter > 1
    {
        return Err("invalid resample rectangle or mode".into());
    }
    // Downsample cell endpoints and midpoint sums are formed in u32; sampling
    // uses signed texel positions. Prove both operations before raw native loads.
    for extent in [config.width, config.height] {
        if extent > i32::MAX as u32
            || (matches!(stage, Resample::Downsample)
                && extent.checked_mul(config.downsample.max(1)).is_none())
        {
            return Err("resample coordinate overflow".into());
        }
    }
    let entry = match stage {
        Resample::Downsample => "filter_downsample_region",
        Resample::Upsample => "filter_upsample_region",
    };
    region::record(
        batch,
        entry,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source)]),
        target,
    )
}
