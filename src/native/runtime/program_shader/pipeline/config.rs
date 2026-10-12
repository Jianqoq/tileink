//! Pixel invocation ABI and bounds shared by fixed and multi-pass frontends.
use super::*;
use crate::native::runtime::compute::Resource;
use crate::shared::filter_config::FilterConfig;

pub(super) fn validate_output(
    batch: &ComputeBatch,
    region: FilterConfig,
    inputs: &[ResourceId],
    output: &super::super::Output,
) -> Result<()> {
    for (id, extent) in std::iter::once((output.image, output.extent))
        .chain(inputs.iter().map(|&id| (id, [region.width, region.height])))
    {
        batch.size(id)?;
        if !matches!(&batch.resources()[id.index()], Resource::Texture(t) if !t.array && t.size[0] >= extent[0] && t.size[1] >= extent[1])
        {
            return Err("shader texture extent mismatch".into());
        }
    }
    let x1 = region
        .region_x0
        .checked_add(region.region_width)
        .ok_or("shader region overflow")?;
    let y1 = region
        .region_y0
        .checked_add(region.region_height)
        .ok_or("shader region overflow")?;
    if x1 > region.width
        || y1 > region.height
        || region.region_x0 < output.origin[0]
        || region.region_y0 < output.origin[1]
        || u64::from(x1) > u64::from(output.origin[0]) + u64::from(output.extent[0])
        || u64::from(y1) > u64::from(output.origin[1]) + u64::from(output.extent[1])
    {
        return Err("shader region exceeds output domain".into());
    }
    Ok(())
}

pub(super) fn pixel_grid(region: FilterConfig, tiles: Option<&[u32]>) -> Result<[u32; 3]> {
    let count = if let Some(tiles) = tiles {
        u32::try_from(tiles.len())?.checked_mul(256)
    } else {
        region.region_width.checked_mul(region.region_height)
    }
    .ok_or("shader pixel count overflow")?;
    if count == 0 || region.region_width == 0 || region.region_height == 0 {
        return Ok([0, 1, 1]);
    }
    let groups = count.div_ceil(256);
    let columns = groups.min(65535);
    let rows = groups.div_ceil(columns);
    if rows > 65535 || u64::from(columns) * u64::from(rows) * 256 > u64::from(u32::MAX) {
        return Err("shader pixel grid exceeds addressing".into());
    }
    Ok([columns, rows, 1])
}

pub(super) fn active_tiles(
    batch: &mut ComputeBatch,
    region: FilterConfig,
    tiles: Option<&[u32]>,
    dispatch: ShaderDispatch,
) -> Result<ResourceId> {
    if matches!(dispatch, ShaderDispatch::Pixels)
        && let Some(tiles) = tiles
    {
        let validated = batch.validate_filter_tiles(
            tiles,
            u64::from(region.width.div_ceil(16)) * u64::from(region.height.div_ceil(16)),
        )?;
        batch.filter_tile_buffer(validated)
    } else {
        batch.buffer(vec![0; 4])
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Explicit geometry/resource parameters preserve the renderer and native ABI contract."
)]
pub(super) fn buffer(
    batch: &mut ComputeBatch,
    pipeline: &ShaderPipeline,
    c: crate::ShaderConfigDescriptor,
    region: FilterConfig,
    tiles: Option<&[u32]>,
    dispatch: ShaderDispatch,
    grid: [u32; 3],
    output: Option<&super::super::Output>,
) -> Result<ResourceId> {
    let [width, height] = c.extent.resolve([region.width, region.height]);
    let [region_x0, region_y0, region_x1, region_y1] = match dispatch {
        ShaderDispatch::Region { outset, .. } => region_bounds(region, c.extent, outset),
        ShaderDispatch::Pixels => [
            region.region_x0,
            region.region_y0,
            region.region_x0 + region.region_width,
            region.region_y0 + region.region_height,
        ],
        _ => [0, 0, width, height],
    };
    let compact = matches!(dispatch, ShaderDispatch::Pixels) && tiles.is_some();
    let active_tile_count = if compact {
        u32::try_from(tiles.map_or(0, <[u32]>::len))?
    } else {
        0
    };
    let pixel_count = if compact {
        active_tile_count.checked_mul(256)
    } else {
        (region_x1 - region_x0).checked_mul(region_y1 - region_y0)
    }
    .ok_or("shader pixel count overflow")?;
    let [
        inverse_xx,
        inverse_yx,
        inverse_xy,
        inverse_yy,
        inverse_tx,
        inverse_ty,
    ] = pipeline.transform;
    let [a, b, cx, d, _, _] = pipeline.transform.map(f64::from);
    let det = a * d - b * cx;
    let [sample_xx, sample_yx, sample_xy, sample_yy] =
        [d / det, -b / det, -cx / det, a / det].map(|n| n as f32);
    let origin = output.map_or([0; 2], |o| o.origin);
    let extent = output.map_or([region.width, region.height], |o| o.extent);
    let config = ShaderConfig {
        width,
        height,
        region_x0,
        region_y0,
        region_width: region_x1 - region_x0,
        region_height: region_y1 - region_y0,
        dispatch_width: grid[0],
        pixel_count,
        compact_tiles: u32::from(compact),
        active_tile_count,
        tiles_width: width.div_ceil(16),
        tiles_height: height.div_ceil(16),
        inverse_xx,
        inverse_yx,
        inverse_xy,
        inverse_yy,
        inverse_tx,
        inverse_ty,
        sample_xx,
        sample_yx,
        sample_xy,
        sample_yy,
        sample_radius: c.sample_radius,
        uniform_words: c.uniform_words,
        output_x0: origin[0],
        output_y0: origin[1],
        output_width: extent[0],
        output_height: extent[1],
    };
    batch.buffer(bytemuck::bytes_of(&config).to_vec())
}
