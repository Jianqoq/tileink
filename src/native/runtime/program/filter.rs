use super::super::{
    Result,
    compute::{ComputeBatch, Resource, ResourceId},
};
use crate::shared::{
    filter_config::FilterConfig,
    gpu_constants::{FILTER_WORKGROUP_SIZE, FINE_WORKGROUP_SIZE, TILE_SIZE},
};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicFilter {
    Clear,
    Copy,
    SourceAlpha,
    Tile,
    Offset,
    DropShadowMask,
}
impl BasicFilter {
    pub fn entry(self) -> &'static str {
        match self {
            Self::Clear => "filter_clear_region",
            Self::Copy => "filter_copy_region",
            Self::SourceAlpha => "filter_source_alpha_region",
            Self::Tile => "filter_tile_region",
            Self::Offset => "filter_offset_region",
            Self::DropShadowMask => "filter_drop_shadow_mask_region",
        }
    }
}

/// Validate logical coordinates and unique write ownership before recording raw native work.
/// Dispatch/list counts are derived here, so stale physical capacity never becomes live pixels.
pub fn encode(
    batch: &mut ComputeBatch,
    kernel: BasicFilter,
    mut config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    for id in [source, target] {
        batch.size(id)?;
        match &batch.resources()[id.index()] {
            Resource::Texture(texture)
                if !texture.array && texture.size == [config.width, config.height] => {}
            _ => return Err("filter requires matching 2D RGBA8 textures".into()),
        }
    }
    if source == target {
        return Err("filter source and destination must be distinct".into());
    }
    let x1 = config
        .region_x0
        .checked_add(config.region_width)
        .ok_or("filter region overflow")?;
    let y1 = config
        .region_y0
        .checked_add(config.region_height)
        .ok_or("filter region overflow")?;
    if x1 > config.width || y1 > config.height {
        return Err("filter region exceeds texture".into());
    }
    for (extent, offset) in [
        (config.width, config.offset_x),
        (config.height, config.offset_y),
    ] {
        // Both addition (shadow) and subtraction (offset) must fit the shader's signed coordinates.
        if i64::from(extent) + i64::from(offset).abs() > i64::from(i32::MAX) {
            return Err("filter signed coordinate overflow".into());
        }
    }
    if kernel == BasicFilter::Tile {
        let rect = [
            config.rect_x0,
            config.rect_y0,
            config.rect_x1,
            config.rect_y1,
        ];
        if rect.iter().any(|v| !v.is_finite() || *v < 0.0)
            || config.rect_x0 > config.rect_x1
            || config.rect_y0 > config.rect_y1
            || f64::from(config.rect_x1) > f64::from(config.width)
            || f64::from(config.rect_y1) > f64::from(config.height)
        {
            return Err("invalid filter tile source rectangle".into());
        }
    }
    config.tiles_width = config.width.div_ceil(TILE_SIZE);
    config.tiles_height = config.height.div_ceil(TILE_SIZE);
    config.compact_tiles = u32::from(tiles.is_some());
    config.active_tile_count = u32::try_from(tiles.map_or(0, |t| t.len()))?;
    config.pixel_count = if let Some(tiles) = tiles {
        let tile_count = u64::from(config.tiles_width) * u64::from(config.tiles_height);
        let mut unique = BTreeSet::new();
        if tiles
            .iter()
            .any(|t| u64::from(*t) >= tile_count || !unique.insert(*t))
        {
            return Err("filter active tiles must be unique and within the surface".into());
        }
        config
            .active_tile_count
            .checked_mul(FINE_WORKGROUP_SIZE)
            .ok_or("filter compact pixel count overflow")?
    } else {
        config
            .region_width
            .checked_mul(config.region_height)
            .ok_or("filter pixel count overflow")?
    };
    if config.region_width == 0 || config.region_height == 0 || config.pixel_count == 0 {
        return Ok(());
    }
    // Bound padded linear invocation arithmetic as well as hardware dispatch dimensions.
    let groups = config.pixel_count.div_ceil(FILTER_WORKGROUP_SIZE);
    if config.dispatch_width > 65535 {
        return Err("filter dispatch width exceeds hardware limit".into());
    }
    config.dispatch_width = if config.dispatch_width == 0 {
        groups.min(65535)
    } else {
        groups.min(config.dispatch_width)
    };
    let rows = groups.div_ceil(config.dispatch_width);
    if rows > 65535
        || u64::from(config.dispatch_width) * u64::from(rows) * u64::from(FILTER_WORKGROUP_SIZE)
            > u64::from(u32::MAX)
    {
        return Err("filter padded dispatch addressing overflow".into());
    }
    let uniform = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let active = batch.buffer(
        tiles
            .filter(|t| !t.is_empty())
            .unwrap_or(&[0])
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    )?;
    let mut bindings = vec![(0, uniform), (3, target), (8, active)];
    if kernel != BasicFilter::Clear {
        bindings.push((1, source));
    }
    // SAFETY: region/source bounds and integer arithmetic are checked above. Distinct textures,
    // unique tiles and injective offset translation give every written pixel exactly one owner.
    unsafe { batch.dispatch(kernel.entry(), &bindings, [config.dispatch_width, rows, 1]) }
}
