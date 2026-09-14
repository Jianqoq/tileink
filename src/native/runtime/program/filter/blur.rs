use super::region::{self, Geometry};
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};

#[derive(Clone, Copy, Debug)]
pub enum Blur {
    Global,
    Shared,
}

pub fn encode(
    batch: &mut ComputeBatch,
    stage: Blur,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    if !config.amount.is_finite() || config.blur_axis > 1 {
        return Err("invalid blur sigma or axis".into());
    }
    let radius = (config.amount.max(0.0) * 3.0).ceil().max(1.0);
    if !radius.is_finite() || f64::from(radius) > f64::from(i32::MAX) {
        return Err("blur radius overflow".into());
    }
    for (extent, lower, upper) in [
        (config.width, config.source_x0, config.source_x1),
        (config.height, config.source_y0, config.source_y1),
    ] {
        if lower > upper
            || upper > extent
            || u64::from(extent) + radius as u64 + u64::from(TILE_SIZE) > i32::MAX as u64
        {
            return Err("blur source bounds or padded coordinate overflow".into());
        }
    }
    let (entry, geometry) = match stage {
        Blur::Global => ("filter_blur_region", Geometry::Pixels),
        Blur::Shared => ("filter_blur_shared_region", Geometry::Tiles),
    };
    region::record(
        batch,
        entry,
        geometry,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source)], [config.width, config.height]),
        target,
    )
}
