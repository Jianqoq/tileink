use super::region;
use crate::native::runtime::Result;
use crate::native::runtime::{
    compute::{ComputeBatch, ResourceId},
    program::filter::FilterConfig,
};

/// Validated immutable path geometry. The private handles preserve range/address invariants.
#[derive(Clone, Copy, Debug)]
pub struct Paths {
    buffers: [ResourceId; 6],
    count: u32,
}

/// Lines are signed 24.8 coordinates [x0,y0,x1,y1]; ranges are half-open line indices.
pub fn upload(
    batch: &mut ComputeBatch,
    ranges: &[std::ops::Range<u32>],
    lines: &[[i32; 4]],
) -> Result<Paths> {
    let count = u32::try_from(ranges.len())?;
    if count == 0 || count > u32::MAX / 4 || lines.len() > (u32::MAX / 4) as usize {
        return Err("path mask raw address capacity exceeded or missing path ranges".into());
    }
    for range in ranges {
        if range.start > range.end || range.end as usize > lines.len() {
            return Err("invalid path mask line range".into());
        }
    }
    let starts = ranges.iter().flat_map(|v| v.start.to_le_bytes()).collect();
    let ends = ranges.iter().flat_map(|v| v.end.to_le_bytes()).collect();
    let mut coordinates = Vec::new();
    for lane in 0..4 {
        let mut bytes: Vec<u8> = lines.iter().flat_map(|v| v[lane].to_le_bytes()).collect();
        // Empty paths still need a nonempty GPU binding; ranges never read this sentinel.
        if bytes.is_empty() {
            bytes.resize(4, 0);
        }
        coordinates.push(batch.buffer(bytes)?);
    }
    Ok(Paths {
        buffers: [
            batch.buffer(starts)?,
            batch.buffer(ends)?,
            coordinates[0],
            coordinates[1],
            coordinates[2],
            coordinates[3],
        ],
        count,
    })
}
pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    paths: Paths,
    target: ResourceId,
) -> Result<()> {
    // Pixel centers must fit the signed fixed-point coordinate domain.
    let scale = u64::from(crate::shared::gpu_constants::PATH_MASK_COORDINATE_SCALE);
    if [config.width, config.height]
        .iter()
        .any(|v| u64::from(*v) * scale > i32::MAX as u64 + 1)
    {
        return Err("path mask target exceeds signed fixed-point coordinates".into());
    }
    if config.table_index >= paths.count {
        return Err("path mask index out of range".into());
    }
    let buffers = std::array::from_fn::<_, 6, _>(|i| (9 + i as u32, paths.buffers[i]));
    region::record(
        batch,
        "filter_path_mask_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings {
            buffers: &buffers,
            ..Default::default()
        },
        target,
    )
}
