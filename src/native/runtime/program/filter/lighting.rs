use super::region;
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::filter_config::FilterConfig,
};
pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    let values = [
        config.surface_scale,
        config.light_constant,
        config.specular_exponent,
        config.light_r,
        config.light_g,
        config.light_b,
        config.light_p0,
        config.light_p1,
        config.light_p2,
        config.light_p3,
        config.light_p4,
        config.light_p5,
        config.light_p6,
        config.light_p7,
    ];
    if config.light_kind > 2
        || config.lighting_output_kind > 1
        || values.iter().any(|v| !v.is_finite())
    {
        return Err("invalid lighting mode or nonfinite parameter".into());
    }
    if config.width >= i32::MAX as u32 || config.height >= i32::MAX as u32 {
        return Err("lighting gradient coordinate overflow".into());
    }
    // Differences and squared vector lengths must remain finite before normalization.
    let limit = f64::from(f32::MAX).sqrt() / 8.0;
    if [
        config.surface_scale,
        config.light_p0,
        config.light_p1,
        config.light_p2,
        config.light_p3,
        config.light_p4,
        config.light_p5,
    ]
    .iter()
    .any(|v| f64::from(v.abs()) > limit)
    {
        return Err("lighting direction magnitude overflow".into());
    }
    region::record(
        batch,
        "filter_lighting_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source)], [config.width, config.height]),
        target,
    )
}
