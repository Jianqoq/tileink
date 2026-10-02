//! Shared transfer fixtures use independently rounded integer table lookup.
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::transfer};
use crate::shared::{
    filter_config::FilterConfig,
    gpu_constants::TILE_SIZE,
    layer::filter::{
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable,
    },
};

pub(super) fn tables() -> Vec<ComponentTransferTable> {
    let mut tables = vec![[0; COMPONENT_TRANSFER_TABLE_LEN]; 3];
    for value in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        for channel in 0..4 {
            let index = channel * COMPONENT_TRANSFER_TABLE_SIZE + value;
            tables[0][index] = value as u32;
            tables[1][index] = if channel == 3 {
                value as u32
            } else {
                255 - value as u32
            };
            tables[2][index] = if channel == 3 {
                255 - value as u32
            } else if value < 128 {
                0
            } else {
                255
            };
        }
    }
    tables
}

fn apply(pixel: &[u8], table: &ComponentTransferTable) -> [u8; 4] {
    let a = u32::from(pixel[3]);
    let alpha = table[3 * COMPONENT_TRANSFER_TABLE_SIZE + a as usize];
    let mut result = [0u8; 4];
    for channel in 0..3 {
        let index = (u32::from(pixel[channel]) * 255 + a / 2)
            .checked_div(a)
            .unwrap_or(0)
            .min(255);
        let value = table[channel * COMPONENT_TRANSFER_TABLE_SIZE + index as usize];
        result[channel] = ((value * alpha + 127) / 255) as u8;
    }
    result[3] = alpha as u8;
    result
}

pub(super) fn case() -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let extent = [33u32, 17];
    let initial: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            [
                (i * 11) as u8,
                (i * 37) as u8,
                (i * 71) as u8,
                (i * 43) as u8,
            ]
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let data = tables();
    let table = transfer::upload(&mut batch, &data)?;
    let tiles = [5, 2, 0];
    let mut expected = Vec::new();
    for compact in [false, true] {
        for index in 0..3 {
            let c = FilterConfig {
                width: extent[0],
                height: extent[1],
                region_x0: 1,
                region_y0: 1,
                region_width: extent[0] - 1,
                region_height: extent[1] - 1,
                dispatch_width: 2,
                table_index: index,
                ..Default::default()
            };
            let target = batch.texture_rgba8(extent, initial.clone())?;
            let active = compact.then_some(tiles.as_slice());
            let mut cpu = initial.clone();
            for table_index in [index, (index + 1) % 3] {
                transfer::encode(
                    &mut batch,
                    FilterConfig { table_index, ..c },
                    active,
                    table,
                    target,
                )?;
                for y in 1..extent[1] {
                    for x in 1..extent[0] {
                        if compact
                            && !tiles.contains(
                                &(y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE) + x / TILE_SIZE),
                            )
                        {
                            continue;
                        }
                        let i = ((y * extent[0] + x) * 4) as usize;
                        let result = apply(&cpu[i..i + 4], &data[table_index as usize]);
                        cpu[i..i + 4].copy_from_slice(&result);
                    }
                }
            }
            batch.readback(target)?;
            expected.push(cpu);
        }
    }
    Ok((batch, expected))
}
