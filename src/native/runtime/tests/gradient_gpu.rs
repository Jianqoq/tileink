use super::four_api::Routes;
use crate::shared::fine_config::FineConfig;
use crate::{
    native::runtime::{Result, compute::ComputeBatch},
    shared::gpu_constants::FINE_WORKGROUP_SIZE,
};

#[derive(Clone, Copy)]
enum Oracle {
    Constant(u32),
    LinearClamp,
    SweepCenter,
}

struct BrushCase {
    offset: u32,
    oracle: Option<Oracle>,
}

fn add_brush(
    paint: &mut Vec<u32>,
    kind: u32,
    extend: u32,
    params: &[f32],
    ramp: &[u32],
) -> BrushCase {
    let offset = paint.len() as u32;
    paint.extend([kind, extend, 21, ramp.len() as u32, 0x80402010, 0, 0, 0, 0]);
    paint.extend(
        params
            .iter()
            .copied()
            .chain(std::iter::repeat(0.0))
            .take(12)
            .map(f32::to_bits),
    );
    paint.extend_from_slice(ramp);
    BrushCase {
        offset,
        oracle: None,
    }
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_gradients_cover_transforms_extensions_and_degenerate_geometry() -> Result<()> {
    let mut paint = Vec::new();
    let mut brushes = Vec::new();
    let ramp = [0xff000000, 0xffffffff];
    brushes.push(add_brush(&mut paint, 1, 0, &[], &[]));
    brushes.last_mut().unwrap().oracle = Some(Oracle::Constant(0x80402010));
    for extend in 0..3u32 {
        for params in [
            [0.0f32, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.25, 2.0, -0.5, 0.75, 0.5, -0.25, 1.25, 0.125, -0.25],
        ] {
            let mut brush = add_brush(&mut paint, 2, extend, &params, &ramp);
            if extend == 0 && params == [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
                brush.oracle = Some(Oracle::LinearClamp);
            }
            brushes.push(brush);
        }
        for params in [
            [
                0.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
            ],
            [
                0.0, 0.0, 0.25, 0.5, 0.5, 1.0, 0.75, 0.5, -0.25, 1.25, 0.125, -0.25,
            ],
            [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        ] {
            brushes.push(add_brush(&mut paint, 3, extend, &params, &ramp));
        }
        for span in [
            0.0f32,
            std::f32::consts::TAU,
            -std::f32::consts::TAU,
            std::f32::consts::PI,
        ] {
            brushes.push(add_brush(
                &mut paint,
                4,
                extend,
                &[0.25, -0.5, 0.0, span],
                &ramp,
            ));
            brushes.last_mut().unwrap().oracle = Some(Oracle::SweepCenter);
        }
    }
    for params in [
        [0.0f32, 0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0, 1.0],
        [0.0, 1.0, 1.0, 1.0],
        [1.0, 1.0, -1.0, -1.0],
    ] {
        brushes.push(add_brush(
            &mut paint,
            5,
            0,
            &params,
            &[0xff0000ff, 0x80008000, 0xffff0000, 0x40000040],
        ));
    }
    // Empty and singleton ramps are independent of geometric interpolation.
    brushes.push(add_brush(
        &mut paint,
        2,
        0,
        &[0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        &[],
    ));
    brushes.last_mut().unwrap().oracle = Some(Oracle::Constant(0));
    brushes.push(add_brush(
        &mut paint,
        2,
        0,
        &[0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        &[0x80402010],
    ));
    brushes.last_mut().unwrap().oracle = Some(Oracle::Constant(0x80402010));
    let coordinates = [
        -2.0f32,
        -1.0,
        -0.5,
        0.25,
        -0.0000001,
        0.0,
        f32::MIN_POSITIVE,
        0.1,
        0.49999997,
        0.5,
        0.50000006,
        0.99999994,
        1.0,
        1.5,
        2.0,
        3.0,
    ];
    let mut requests = Vec::<u32>::new();
    let mut independently_checked = Vec::new();
    for brush in &brushes {
        for &x in &coordinates {
            for &y in &coordinates {
                let index = requests.len() / 4;
                requests.extend([brush.offset, x.to_bits(), y.to_bits(), 0]);
                let oracle = match brush.oracle {
                    Some(Oracle::Constant(color)) => Some(color),
                    Some(Oracle::LinearClamp) => {
                        let channel = (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
                        Some(0xff000000 | (channel * 0x010101))
                    }
                    Some(Oracle::SweepCenter) if x == 0.25 && y == -0.5 => Some(0xff000000),
                    _ => None,
                };
                if let Some(value) = oracle {
                    independently_checked.push((index, value));
                }
            }
        }
    }
    // A five-stop ramp exercises all intervals and both sides of internal knots.
    let colors = [0xff0000ff, 0xff00ff00, 0xffff0000, 0xffffffff, 0xff000000];
    for extend in 0..3 {
        let brush = add_brush(
            &mut paint,
            2,
            extend,
            &[0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            &colors,
        );
        for (knot, &color) in colors.iter().enumerate() {
            let x = knot as f32 * 0.25;
            for sample in [x.next_down(), x, x.next_up()] {
                let index = requests.len() / 4;
                requests.extend([brush.offset, sample.to_bits(), 0, 0]);
                if sample == x {
                    let expected = if extend == 1 && knot == colors.len() - 1 {
                        colors[0]
                    } else {
                        color
                    };
                    independently_checked.push((index, expected));
                }
            }
        }
    }
    // Both directions cross the atan2 branch cut. Axis endpoints are independent.
    for (start, end, first, last) in [
        (
            std::f32::consts::FRAC_PI_2,
            3.0 * std::f32::consts::FRAC_PI_2,
            [0.0f32, 1.0],
            [0.0f32, -1.0],
        ),
        (
            -std::f32::consts::FRAC_PI_2,
            -3.0 * std::f32::consts::FRAC_PI_2,
            [0.0, -1.0],
            [0.0, 1.0],
        ),
    ] {
        for extend in 0..3 {
            let brush = add_brush(&mut paint, 4, extend, &[0.0, 0.0, start, end], &ramp);
            for [x, y] in [
                first,
                last,
                [-1.0, 0.0],
                [-1.0, -0.0000001],
                [-1.0, 0.0000001],
            ] {
                let index = requests.len() / 4;
                requests.extend([brush.offset, x.to_bits(), y.to_bits(), 0]);
                if [x, y] == first {
                    independently_checked.push((index, ramp[0]));
                }
                if [x, y] == last {
                    independently_checked
                        .push((index, if extend == 1 { ramp[0] } else { ramp[1] }));
                }
            }
        }
    }
    let count = requests.len() / 4;
    // Use the production FineConfig uniform layout and a nonzero paint base.
    let config = FineConfig {
        paint_brush_base: 13,
        ..Default::default()
    };
    let paint: Vec<u32> = [0xaabbccddu32; 13].into_iter().chain(paint).collect();
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let paint = batch.buffer(bytes(&paint))?;
    let requests = batch.buffer(bytes(&requests))?;
    let request_config = batch.buffer(bytes(&[count as u32, 0, 0, 0]))?;
    let output = batch.buffer(bytes(&vec![0x41424344; count + 4]))?;
    // SAFETY: all brush headers, parameters and payload ranges are complete;
    // the adapter rejects padded lanes before any access.
    unsafe {
        batch.dispatch(
            "gradient_words",
            &[
                (0, config),
                (3, paint),
                (9, requests),
                (10, output),
                (11, request_config),
            ],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let routes = Routes::new()?;
    let expected = routes.reference_output(&batch)?;
    for (index, value) in independently_checked {
        assert_eq!(
            u32::from_le_bytes(expected[0][index * 4..index * 4 + 4].try_into().unwrap()),
            value,
            "gradient oracle {index}"
        );
    }
    assert_eq!(
        &expected[0][count * 4..],
        bytes(&[0x41424344; 4]).as_slice()
    );
    routes.check(&batch, &expected, "gradient packed pixels")?;
    routes.validate()
}
