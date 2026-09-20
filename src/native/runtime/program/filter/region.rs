use super::{ComputeBatch, FilterConfig, ResourceId, Result};
use crate::native::runtime::compute::Resource;
use crate::shared::gpu_constants::{FILTER_WORKGROUP_SIZE, FINE_WORKGROUP_SIZE, TILE_SIZE};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub(super) enum Geometry {
    Pixels,
    Tiles,
}

// Texture extent checks and raw-buffer checks are separate. Stage-owned typed data
// validates buffer contents/address ranges before supplying these read bindings.
#[derive(Default)]
pub(super) struct ReadBindings<'a> {
    pub textures: &'a [(u32, ResourceId)],
    pub texture_extent: [u32; 2],
    pub buffers: &'a [(u32, ResourceId)],
    pub images: Option<&'a crate::native::runtime::program::scene::SceneImages>,
}
impl<'a> ReadBindings<'a> {
    pub fn textures(textures: &'a [(u32, ResourceId)], extent: [u32; 2]) -> Self {
        Self {
            textures,
            texture_extent: extent,
            buffers: &[],
            images: None,
        }
    }
}

// Shared logical bounds and write ownership for all filter stages. Resource capacities
// never define live pixels; only validated region/list counts do.
pub(super) fn record(
    batch: &mut ComputeBatch,
    entry: &'static str,
    geometry: Geometry,
    mut config: FilterConfig,
    tiles: Option<&[u32]>,
    reads: ReadBindings<'_>,
    target: ResourceId,
) -> Result<()> {
    if config.rounding_zero.to_bits() != 0 {
        return Err("filter rounding operand must be positive zero".into());
    }
    validate_texture(batch, target, [config.width, config.height])?;
    for (_, id) in reads.textures {
        validate_texture(batch, *id, reads.texture_extent)?;
    }
    for (_, id) in reads.buffers {
        batch.size(*id)?;
        if !matches!(
            &batch.resources()[id.index()],
            Resource::Buffer(_) | Resource::PersistentBuffer(_)
        ) {
            return Err("filter table binding requires a buffer".into());
        }
    }
    if let Some(images) = reads.images {
        for id in [images.atlas, images.table, images.sampler] {
            batch.size(id)?;
        }
        if let Resource::TextureTable(textures) = &batch.resources()[images.table.index()]
            && textures.contains(&target)
        {
            return Err("filter image table aliases destination".into());
        }
        if images.atlas == target {
            return Err("filter atlas aliases destination".into());
        }
    }
    if reads.textures.iter().any(|(_, id)| *id == target) {
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
    if config.dispatch_width > 65535 {
        return Err("filter dispatch width exceeds hardware limit".into());
    }
    let (columns, rows, lanes) = match geometry {
        Geometry::Tiles if tiles.is_none() => (
            config.region_width.div_ceil(TILE_SIZE),
            config.region_height.div_ceil(TILE_SIZE),
            1u64,
        ),
        _ => {
            let groups = match geometry {
                Geometry::Pixels => config.pixel_count.div_ceil(FILTER_WORKGROUP_SIZE),
                Geometry::Tiles => config.active_tile_count,
            };
            let columns = groups.min(if config.dispatch_width == 0 {
                65535
            } else {
                config.dispatch_width
            });
            (
                columns,
                groups.div_ceil(columns),
                if matches!(geometry, Geometry::Pixels) {
                    u64::from(FILTER_WORKGROUP_SIZE)
                } else {
                    1
                },
            )
        }
    };
    if columns > 65535
        || rows > 65535
        || u64::from(columns) * u64::from(rows) * lanes > u64::from(u32::MAX)
    {
        return Err("filter padded dispatch addressing overflow".into());
    }
    config.dispatch_width = columns;
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
    bindings.extend_from_slice(reads.textures);
    bindings.extend_from_slice(reads.buffers);
    if let Some(images) = reads.images {
        bindings.extend([(12, images.atlas), (13, images.sampler), (30, images.table)]);
    }
    // SAFETY: region/source bounds are checked above; each stage validates its table addresses. Distinct textures,
    // unique tiles and injective offset translation give every written pixel exactly one owner.
    unsafe { batch.dispatch(entry, &bindings, [config.dispatch_width, rows, 1]) }
}

// Validate the logical image against its allocation, independently for each pass's
// source and target. Pooled padding is storage, never part of the sampled domain.
fn validate_texture(batch: &ComputeBatch, id: ResourceId, extent: [u32; 2]) -> Result<()> {
    batch.size(id)?;
    match &batch.resources()[id.index()] {
        Resource::Texture(texture)
            if !texture.array && texture.size[0] >= extent[0] && texture.size[1] >= extent[1] =>
        {
            Ok(())
        }
        _ => Err("filter requires a 2D RGBA8 texture containing its logical domain".into()),
    }
}
