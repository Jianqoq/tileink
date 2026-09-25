use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};
use crate::shared::layer::filter::ComponentTransferTable;

#[derive(Clone, Copy, Debug)]
pub struct TransferTables {
    buffer: ResourceId,
    count: u32,
}

/// Validate table values and all raw byte addresses once, before GPU use.
pub fn upload(
    batch: &mut ComputeBatch,
    tables: &[ComponentTransferTable],
) -> Result<TransferTables> {
    if tables.is_empty() {
        return Err("component transfer requires at least one table".into());
    }
    let size = tables
        .len()
        .checked_mul(std::mem::size_of::<ComponentTransferTable>())
        .filter(|n| u32::try_from(*n).is_ok())
        .ok_or("component transfer byte addressing overflow")?;
    let mut bytes = Vec::with_capacity(size);
    for value in tables.iter().flatten() {
        if *value > u32::from(u8::MAX) {
            return Err("component transfer values must be bytes".into());
        }
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(TransferTables {
        buffer: batch.buffer(bytes)?,
        count: u32::try_from(tables.len())?,
    })
}

pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    tables: TransferTables,
    target: ResourceId,
) -> Result<()> {
    if config.table_index >= tables.count {
        return Err("component transfer table index out of range".into());
    }
    region::record(
        batch,
        "filter_component_transfer_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings {
            buffers: &[(7, tables.buffer)],
            ..Default::default()
        },
        target,
    )
}
