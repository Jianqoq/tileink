use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{Result, compute::ComputeBatch},
    shared::{
        filter_config::FilterConfig,
        gpu_constants::{FILTER_WORKGROUP_SIZE, SDF_PROBE_REQUEST_WORDS, SDF_RECORD_WORDS},
    },
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_sdf_shapes_transforms_and_boundaries() -> Result<()> {
    let mut records = Vec::<u32>::new();
    let mut requests = Vec::<u32>::new();
    let mut oracle = Vec::<u32>::new();
    // Independent identity rectangle/circle distance and one-device-pixel ramp.
    for kind in [1u32, 2] {
        let offset = records.len() as u32;
        let values = if kind == 1 {
            [2.0f32, 2.0, 10.0, 8.0]
        } else {
            [6.0, 6.0, 4.0, 0.0]
        };
        records.push(kind);
        records.extend(values.map(f32::to_bits));
        records.extend([0; SDF_RECORD_WORDS as usize - 5]);
        for phase in [0.0f32, 0.5] {
            for y in 0..12 {
                for x in 0..14 {
                    let px = x as f32 + phase;
                    let py = y as f32 + phase;
                    requests.extend([
                        offset,
                        px.to_bits(),
                        py.to_bits(),
                        0,
                        1.0f32.to_bits(),
                        0,
                        0,
                        1.0f32.to_bits(),
                        0,
                        0,
                        0,
                        0,
                    ]);
                    let distance = if kind == 1 {
                        let qx = (f64::from(px) - 6.0).abs() - 4.0;
                        let qy = (f64::from(py) - 5.0).abs() - 3.0;
                        qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0))
                    } else {
                        (f64::from(px) - 6.0).hypot(f64::from(py) - 6.0) - 4.0
                    };
                    oracle.push(((0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u32);
                }
            }
        }
    }
    let checked = oracle.len();
    let transforms = [
        [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0],
        [0.5, 0.0, 0.0, 2.0, 1.25, -0.75],
        [-1.0, 0.0, 0.0, 1.0, 17.0, 0.0],
        [0.8, 0.6, -0.6, 0.8, 3.25, -2.5],
        [1.0, 0.25, 0.75, 1.0, -2.0, 1.0],
    ];
    for kind in 1..=19u32 {
        let mut values = [
            2.0f32, 3.0, 12.0, 11.0, 2.0, 1.0, 3.0, 2.0, 1.0, 2.0, 1.0, 1.0, 0.75, -0.5, 2.5, 0.7,
        ];
        match kind {
            2 | 4 | 10 => {
                values[0] = 8.0;
                values[1] = 8.0;
                values[2] = 5.0;
            }
            8 | 9 => {
                values[0] = 8.0;
                values[1] = 8.0;
                values[2] = 5.0;
                values[3] = 2.0;
                values[4] = -0.75;
                values[5] = 4.5;
                values[6] = 2.0;
            }
            15 | 16 => {
                values[0] = 8.0;
                values[1] = 8.0;
                values[2] = 6.0;
                values[3] = 3.0;
                values[4] = 0.5;
                values[5] = 0.3;
            }
            _ => {}
        }
        let mut geometries = vec![values];
        match kind {
            6 | 11 | 12 => {
                for cap in [0.0, 1.0, 2.0] {
                    for degenerate in [false, true] {
                        let mut v = values;
                        v[5] = cap;
                        if degenerate {
                            v[2] = v[0];
                            v[3] = v[1];
                        }
                        geometries.push(v);
                    }
                }
            }
            8 | 9 => {
                for cap in [0.0, 1.0, 2.0] {
                    for sweep in [-6.2831855, -2.25, 0.0, 6.2831855] {
                        let mut v = values;
                        v[6] = cap;
                        v[5] = sweep;
                        geometries.push(v);
                    }
                }
            }
            17..=19 => {
                for side in [0.0, 1.0, 2.0, 3.0] {
                    for hidden in [false, true] {
                        let mut v = values;
                        v[9] = side;
                        if hidden {
                            v[11] = 0.0;
                        }
                        geometries.push(v);
                    }
                }
            }
            1 | 3 | 7 => {
                let mut v = values;
                v.swap(0, 2);
                v.swap(1, 3);
                geometries.push(v);
            }
            2 | 4 | 10 => {
                let mut v = values;
                v[2] = 0.0;
                geometries.push(v);
            }
            14 => {
                let mut v = values;
                v[4] = 0.0;
                geometries.push(v);
            }
            15 | 16 => {
                let mut v = values;
                v[2] = 0.0;
                v[3] = 0.0;
                geometries.push(v);
            }
            _ => {}
        }
        for values in geometries {
            let offset = records.len() as u32;
            records.push(kind);
            records.extend(values.map(f32::to_bits));
            for transform in transforms {
                for phase in [0.0f32, 0.5] {
                    for y in 0..19 {
                        for x in 0..21 {
                            requests.extend([
                                offset,
                                (x as f32 - 2.0 + phase).to_bits(),
                                (y as f32 - 2.0 + phase).to_bits(),
                                0,
                            ]);
                            requests.extend(transform.map(f32::to_bits));
                            requests.extend([0, 0]);
                        }
                    }
                }
            }
        }
    }
    let bytes = |v: &[u32]| v.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
    let count = (requests.len() / SDF_PROBE_REQUEST_WORDS as usize) as u32;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        bytemuck::bytes_of(&FilterConfig {
            pixel_count: count,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let paint = batch.buffer(bytes(&records))?;
    let positions = batch.buffer(bytes(&requests))?;
    let output = batch.buffer(bytes(&vec![0xa1b2c3d4; count as usize + 4]))?;
    // SAFETY: each request addresses a complete 17-word shape; logical count
    // excludes four output sentinels and extra dispatch groups exercise tail guards.
    unsafe {
        batch.dispatch(
            "sdf_coverage_words",
            &[(0, config), (5, positions), (6, output), (7, paint)],
            [count.div_ceil(FILTER_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let variant = FilterVariant {
        portable: false,
        texture_table: false,
    };
    let expected = routes.filter_reference_output(&batch, variant)?;
    assert_eq!(&expected[0][..checked * 4], bytes(&oracle));
    assert_eq!(&expected[0][count as usize * 4..], bytes(&[0xa1b2c3d4; 4]));
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "SDF geometry and transformed antialiasing",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
