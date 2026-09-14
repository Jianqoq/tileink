use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::shared::{
    filter_config::FilterConfig,
    gpu_constants::{FINE_WORKGROUP_SIZE, NATIVE_TEXTURE_TABLE_CAPACITY},
};

#[test]
fn texture_table_dispatch_rejects_count_mismatch_and_hidden_write_alias() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let requests = batch.buffer(vec![0; 12])?;
    let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let output = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let short = batch.texture_table(&[image])?;
    let aliased = batch.texture_table(&vec![output; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
    for table in [short, aliased] {
        // SAFETY: recording must reject descriptor count/alias before GPU work;
        // the single request itself has a valid image index and texel coordinate.
        assert!(
            unsafe {
                batch.dispatch(
                    "texture_table_words",
                    &[(0, requests), (1, output), (30, table)],
                    [1, 1, 1],
                )
            }
            .is_err()
        );
    }
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_texture_tables_index_every_image_and_track_member_writes() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut images = Vec::new();
    let mut pixels = Vec::new();
    for image in 0..NATIVE_TEXTURE_TABLE_CAPACITY {
        let size = [2 + image % 3, 1 + image % 4];
        let bytes: Vec<u8> = (0..size[1])
            .flat_map(|y| {
                (0..size[0]).flat_map(move |x| [image as u8, (x * 31) as u8, (y * 47) as u8, 255])
            })
            .collect();
        images.push(batch.texture_rgba8(size, bytes.clone())?);
        pixels.push((size, bytes));
    }
    let table = batch.texture_table(&images)?;
    let count = NATIVE_TEXTURE_TABLE_CAPACITY * 2;
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    let mut changed = Vec::new();
    for i in 0..count {
        let index = i * 17 % NATIVE_TEXTURE_TABLE_CAPACITY;
        let (size, bytes) = &pixels[index as usize];
        let x = i % size[0];
        let y = i / size[0] % size[1];
        requests.extend([index, x, y]);
        let offset = ((y * size[0] + x) * 4) as usize;
        expected.extend_from_slice(&bytes[offset..offset + 4]);
        changed.extend_from_slice(if index == NATIVE_TEXTURE_TABLE_CAPACITY - 1 {
            &[7, 11, 13, 255]
        } else {
            &bytes[offset..offset + 4]
        });
    }
    let requests = batch.buffer(bytemuck::cast_slice(&requests).to_vec())?;
    let first = batch.texture_rgba8([count, 1], vec![0; count as usize * 4])?;
    let second = batch.texture_rgba8([count, 1], vec![0; count as usize * 4])?;
    // SAFETY: every request selects a live table member and a valid texel;
    // the logical request count fits each independent destination row.
    unsafe {
        batch.dispatch(
            "texture_table_words",
            &[(0, requests), (1, first), (30, table)],
            [count.div_ceil(FINE_WORKGROUP_SIZE), 1, 1],
        )?;
    }
    let last = NATIVE_TEXTURE_TABLE_CAPACITY as usize - 1;
    let size = pixels[last].0;
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        FilterConfig {
            width: size[0],
            height: size[1],
            region_width: size[0],
            region_height: size[1],
            clear_color: u32::from_le_bytes([7, 11, 13, 255]),
            ..Default::default()
        },
        None,
        None,
        images[last],
    )?;
    // SAFETY: same checked requests and distinct output, after the table member's write.
    unsafe {
        batch.dispatch(
            "texture_table_words",
            &[(0, requests), (1, second), (30, table)],
            [count.div_ceil(FINE_WORKGROUP_SIZE), 1, 1],
        )?;
    }
    batch.readback(first)?;
    batch.readback(second)?;
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &[expected.clone(), changed.clone()],
                "texture table nonuniform indices and member hazards",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
