use super::four_api::Routes;
use crate::{
    native::runtime::{Result, compute::ComputeBatch},
    shared::gpu_constants::FINE_WORKGROUP_SIZE,
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_geometry_math_preserves_edge_coverage_and_pattern_cells() -> Result<()> {
    // Regression: cancellation in p0y + (p1y - p0y) reaches the half-alpha
    // boundary; preserving each subtraction/addition is observable in RGBA8.
    let mut records = vec![
        (-0.0f32).to_bits(),
        7.75f32.to_bits(),
        1.0000001f32.to_bits(),
        0.50000006f32.to_bits(),
        (-0.5f32).to_bits(),
        0,
        0,
        1,
    ];
    records.extend([
        0.1f32.to_bits(),
        (-0.1f32).to_bits(),
        0.3f32.to_bits(),
        0.3f32.to_bits(),
        0,
        0,
        0,
        0,
    ]);
    let coordinates = [
        -1.0f32, -0.0, 0.0000001, 0.1, 0.49999997, 0.5, 0.50000006, 0.99999994, 1.0, 1.0000001,
        7.75, 15.5, 16.0, 17.0,
    ];
    for (i, &left) in coordinates.iter().enumerate() {
        for (j, &right) in coordinates.iter().enumerate() {
            for row in 0..16u32 {
                for x in 0..16u32 {
                    let top = coordinates[(i + j) % coordinates.len()];
                    let bottom = coordinates[(i * 3 + j * 5) % coordinates.len()];
                    records.extend([
                        left.to_bits(),
                        top.to_bits(),
                        right.to_bits(),
                        bottom.to_bits(),
                        (-0.5f32).to_bits(),
                        row,
                        x,
                        (i % 2) as u32,
                    ]);
                }
            }
        }
    }
    let count = records.len() / 8;
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytes(&[count as u32, 0, 0, 0]))?;
    let source = batch.buffer(bytes(&records))?;
    let guard = 0xa1b2c3d4u32;
    let destination = batch.buffer(bytes(&vec![guard; (count + 2) * 4]))?;
    // SAFETY: records and destinations match the adapter strides; padded groups
    // are intentional and must leave the two trailing records untouched.
    unsafe {
        batch.dispatch(
            "geometry_math_words",
            &[(0, config), (1, source), (2, destination)],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(destination)?;
    let routes = Routes::new()?;
    let expected = routes.reference_output(&batch)?;
    assert_eq!(
        u32::from_le_bytes(expected[0][24..28].try_into().unwrap()),
        0,
        "exact-zero rotated pattern cell"
    );
    assert_eq!(
        u32::from_le_bytes(expected[0][12..16].try_into().unwrap()),
        127,
        "direct endpoint half-alpha regression"
    );
    assert_eq!(&expected[0][count * 16..], bytes(&[guard; 8]).as_slice());
    routes.check(&batch, &expected, "coverage and nearest pattern cells")?;
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fill_coverage_accumulates_segments_in_painter_order() -> Result<()> {
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let segments = [
        [0.5f32, 0.0, 0.5, 16.0, 100.0],
        [0.5, 16.0, 0.5, 0.0, 100.0],
        [-1.0, -0.25, 16.25, 15.5, 100.0],
        [16.25, 15.5, -1.0, -0.25, 100.0],
        [0.5, 1.0, 0.5, 0.0, 100.0],
        [0.5, f32::EPSILON / 2.0, 0.5, 0.0, 100.0],
        [0.5, 0.0, 0.5, f32::EPSILON / 2.0, 100.0],
        [0.5, 1.0, 0.5, 0.0, 100.0],
        [0.5, 0.0, 0.5, f32::EPSILON / 2.0, 100.0],
        [0.5, f32::EPSILON / 2.0, 0.5, 0.0, 100.0],
    ];
    let mut requests = Vec::<u32>::new();
    let mut oracle = Vec::<u32>::new();
    for start in 0..=2u32 {
        for end in start..=2 {
            for backdrop in -3..=3i32 {
                for rule in 0..2u32 {
                    for y in 0..16u32 {
                        for x in 0..16u32 {
                            requests.extend([start, end, backdrop as u32, x, y, rule, 0, 0]);
                            let area = if x == 0 { 0.5f32 } else { 1.0 };
                            let down = if start == 0 && end > 0 { area } else { 0.0 };
                            let up = if start <= 1 && end > 1 { area } else { 0.0 };
                            let winding = (backdrop as f32 - down + up).abs();
                            let alpha = if rule == 0 {
                                winding.min(1.0)
                            } else {
                                let period = winding % 2.0;
                                if period <= 1.0 { period } else { 2.0 - period }
                            };
                            oracle.push((alpha * 255.0 + 0.5) as u32);
                        }
                    }
                }
            }
        }
    }
    // These equal real-number sums differ in f32 when the input order changes.
    // Keeping the two explicit byte expectations detects reassociation of the
    // production partial-coverage accumulator at the half-alpha boundary.
    requests.extend([4, 7, 0, 0, 0, 0, 0, 0]);
    oracle.push(127);
    requests.extend([7, 10, 0, 0, 0, 0, 0, 0]);
    oracle.push(128);
    let checked = oracle.len();
    for end in 1..=4u32 {
        for rule in 0..2u32 {
            for y in 0..16u32 {
                for x in 0..16u32 {
                    requests.extend([0, end, 0, x, y, rule, 0, 0]);
                }
            }
        }
    }
    let count = requests.len() / 8;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytes(&[count as u32, 0, 0, 0]))?;
    let source = batch.buffer(bytes(&requests))?;
    let destination = batch.buffer(bytes(&vec![0xa1b2c3d4; count + 4]))?;
    let segment_buffer = batch.buffer(bytes(
        &segments
            .into_iter()
            .flatten()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
    ))?;
    // SAFETY: every request range lies inside ten complete segment records.
    unsafe {
        batch.dispatch(
            "fill_coverage_words",
            &[
                (0, config),
                (1, source),
                (2, destination),
                (3, segment_buffer),
            ],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(destination)?;
    let routes = Routes::new()?;
    let expected = routes.reference_output(&batch)?;
    assert_eq!(&expected[0][..checked * 4], bytes(&oracle).as_slice());
    assert_eq!(
        &expected[0][count * 4..],
        bytes(&[0xa1b2c3d4; 4]).as_slice()
    );
    routes.check(&batch, &expected, "ordered segment coverage")?;
    routes.validate()
}
