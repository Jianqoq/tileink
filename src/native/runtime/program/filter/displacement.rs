use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};

pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    map: ResourceId,
    target: ResourceId,
) -> Result<()> {
    if config.kernel_edge_mode > 3 || config.kernel_preserve_alpha > 3 {
        return Err("invalid displacement channel".into());
    }
    if !config.amount.is_finite() || !config.rect_x0.is_finite() {
        return Err("nonfinite displacement scale".into());
    }
    region::record(
        batch,
        "filter_displacement_map_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source), (2, map)], [config.width, config.height]),
        target,
    )
}
