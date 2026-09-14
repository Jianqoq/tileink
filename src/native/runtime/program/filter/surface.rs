use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};

/// Composite a logical source surface at its signed destination origin.
/// The source has its own dimensions; no allocation padding may become live pixels.
pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    for (extent, offset, source_extent) in [
        (config.width, config.offset_x, config.kernel_columns),
        (config.height, config.offset_y, config.kernel_rows),
    ] {
        if i64::from(extent) + i64::from(offset).abs() > i64::from(i32::MAX)
            || source_extent > i32::MAX as u32
        {
            return Err("surface composite signed coordinate overflow".into());
        }
    }
    region::record(
        batch,
        "filter_composite_surface_direct_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source)], [config.kernel_columns, config.kernel_rows]),
        target,
    )
}
