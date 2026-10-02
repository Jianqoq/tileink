use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};

/// Morphology samples the full source axis, while region/tiles restrict writes.
pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    if config.morphology_axis > 1 || config.morphology_operator > 1 {
        return Err("invalid morphology axis or operator".into());
    }
    let extent = if config.morphology_axis == 0 {
        config.width
    } else {
        config.height
    };
    // The shader forms pos+radius before clipping. Reject overflow rather than
    // capping the radius, which would change erosion at transparent boundaries.
    if config
        .morphology_radius
        .checked_add(extent.saturating_sub(1))
        .is_none()
    {
        return Err("morphology radius coordinate overflow".into());
    }
    region::record(
        batch,
        "filter_morphology_axis_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source)], [config.width, config.height]),
        target,
    )
}
