use super::region;
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::shared::{
    filter_config::FilterConfig,
    layer::filter::{TURBULENCE_GRADIENT_LEN, TURBULENCE_LATTICE_SIZE, TurbulenceLattice},
};
#[derive(Clone, Copy, Debug)]
pub struct Tables {
    selectors: ResourceId,
    gradients: ResourceId,
    count: u32,
}
/// Validate immutable lattice data and raw byte capacities before any GPU upload.
pub fn upload(batch: &mut ComputeBatch, tables: &[TurbulenceLattice]) -> Result<Tables> {
    if tables.is_empty()
        || tables
            .len()
            .checked_mul(TURBULENCE_GRADIENT_LEN * 4)
            .is_none_or(|n| n > u32::MAX as usize)
    {
        return Err("invalid turbulence table capacity".into());
    }
    for table in tables {
        if table.gradients.len() != TURBULENCE_GRADIENT_LEN
            || table
                .gradients
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1.0)
            || table
                .selectors
                .iter()
                .any(|v| *v >= TURBULENCE_LATTICE_SIZE as u32)
        {
            return Err("invalid turbulence lattice".into());
        }
    }
    let selectors = batch.buffer(
        tables
            .iter()
            .flat_map(|t| t.selectors.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
    )?;
    let gradients = batch.buffer(
        tables
            .iter()
            .flat_map(|t| t.gradients.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
    )?;
    Ok(Tables {
        selectors,
        gradients,
        count: u32::try_from(tables.len())?,
    })
}
pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    tables: Tables,
    target: ResourceId,
) -> Result<()> {
    validate(config, tables.count)?;
    region::record(
        batch,
        "filter_turbulence_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings {
            buffers: &[(5, tables.selectors), (6, tables.gradients)],
            ..Default::default()
        },
        target,
    )
}
fn validate(c: FilterConfig, count: u32) -> Result<()> {
    let values = [
        c.turbulence_base_frequency_x,
        c.turbulence_base_frequency_y,
        c.turbulence_transform_x,
        c.turbulence_transform_y,
        c.turbulence_scale_x,
        c.turbulence_scale_y,
        c.turbulence_tile_x,
        c.turbulence_tile_y,
        c.turbulence_tile_width,
        c.turbulence_tile_height,
    ];
    if c.table_index >= count
        || values.iter().any(|v| !v.is_finite())
        || c.turbulence_kind > 1
        || c.turbulence_stitch_tiles > 1
        || c.linear_rgb > 1
        || c.turbulence_base_frequency_x < 0.0
        || c.turbulence_base_frequency_y < 0.0
    {
        return Err("invalid turbulence configuration".into());
    }
    // A zero scale bypasses noise in the shader. Otherwise every used lattice
    // coordinate and stitch extent must remain inside signed raw-index arithmetic.
    if c.turbulence_scale_x.abs() <= f32::EPSILON
        || c.turbulence_scale_y.abs() <= f32::EPSILON
        || c.turbulence_num_octaves == 0
        || (c.turbulence_base_frequency_x == 0.0 && c.turbulence_base_frequency_y == 0.0)
    {
        return Ok(());
    }
    let octaves = c
        .turbulence_num_octaves
        .min(crate::shared::gpu_constants::TURBULENCE_MAX_EFFECTIVE_OCTAVES);
    let factor = 2.0f64.powi(octaves as i32 - 1);
    for (size, frequency, transform, scale, tile_origin, tile_size) in [
        (
            c.width,
            c.turbulence_base_frequency_x,
            c.turbulence_transform_x,
            c.turbulence_scale_x,
            c.turbulence_tile_x,
            c.turbulence_tile_width,
        ),
        (
            c.height,
            c.turbulence_base_frequency_y,
            c.turbulence_transform_y,
            c.turbulence_scale_y,
            c.turbulence_tile_y,
            c.turbulence_tile_height,
        ),
    ] {
        let size = f64::from(size);
        let scale = f64::from(scale);
        let max_sample = (f64::from(transform)
            .abs()
            .max((size - f64::from(transform)).abs()))
            / scale.abs();
        let mut frequency = f64::from(frequency);
        let offset = f64::from(crate::shared::gpu_constants::TURBULENCE_COORDINATE_OFFSET);
        if c.turbulence_stitch_tiles == 1 {
            if tile_size <= 0.0 {
                return Err("turbulence stitch dimensions must be positive".into());
            }
            let translated = f64::from(tile_origin) - f64::from(transform);
            let origin = translated / scale;
            let delta = f64::from(tile_size) / scale;
            let end = origin + delta;
            let tile = delta.abs();
            if translated.abs() > f64::from(f32::MAX)
                || [origin, delta, end]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > f64::from(f32::MAX))
                || (tile as f32) == 0.0
            {
                return Err("turbulence tile transform overflow or underflow".into());
            }
            if frequency != 0.0 {
                // Bound either snapped frequency and all octave lattice updates.
                frequency = (tile * frequency).ceil() / tile;
                let extent = (tile * frequency + 1.0) * factor;
                let local = origin.abs().max(end.abs());
                if (extent + (local * frequency + 1.0) * factor) * 1.001 + offset
                    >= f64::from(i32::MAX) - 1024.0
                {
                    return Err("turbulence stitch coordinate overflow".into());
                }
            }
        }
        if !max_sample.is_finite()
            || max_sample > f64::from(f32::MAX)
            || frequency * factor > f64::from(f32::MAX)
            || max_sample * frequency * factor * 1.001 + offset >= f64::from(i32::MAX) - 1024.0
        {
            return Err("turbulence sample coordinate overflow".into());
        }
    }
    Ok(())
}
