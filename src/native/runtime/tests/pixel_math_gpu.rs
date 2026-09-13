use super::four_api::Routes;
use crate::native::runtime::{Result, compute::ComputeBatch};

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
fn pack(channels: [u32; 4]) -> u32 {
    channels
        .into_iter()
        .enumerate()
        .fold(0, |color, (channel, value)| color | value << (channel * 8))
}
fn channel(color: u32, index: u32) -> u32 {
    (color >> (index * 8)) & 255
}
fn multiply(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_pixel_math_matches_channel_oracles_and_quantization_boundaries() -> Result<()> {
    let mut cases = Vec::new();
    for a in 0..256u32 {
        for b in 0..256u32 {
            let source = pack([a, a / 2, a / 3, a]);
            let destination = pack([b / 3, b, b / 2, b]);
            let coverage = (a as f32 - 128.0) / 16.0;
            let t = if b < 128 {
                b as f32 / 128.0
            } else {
                (b - 128) as f32 / 254.0
            };
            cases.push([
                a,
                b,
                source,
                destination,
                coverage.to_bits(),
                t.to_bits(),
                b % 2,
                0,
            ]);
        }
    }
    for coverage in [
        -0.0f32,
        0.0,
        f32::MIN_POSITIVE,
        0.5 - f32::EPSILON,
        0.5,
        0.5 + f32::EPSILON,
        1.0,
        -1.0,
    ] {
        for t in [-1.0f32, 0.0, 0.5, 0.50000006, 1.0, 2.0] {
            cases.push([
                1,
                2,
                0x00010203,
                0xffc08040,
                coverage.to_bits(),
                t.to_bits(),
                1,
                0,
            ]);
        }
    }
    let count = cases.len();
    let input: Vec<u32> = cases
        .iter()
        .flatten()
        .copied()
        .chain([0xabababab; 16])
        .collect();
    let initial = vec![0x41424344; (count + 2) * 14];
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytes(&[count as u32, 0, 0, 0]))?;
    let source = batch.buffer(bytes(&input))?;
    let destination = batch.buffer(bytes(&initial))?;
    // SAFETY: complete input/output records and explicit count; padded lanes
    // must preserve the two output guard records.
    unsafe {
        batch.dispatch(
            "pixel_math_words",
            &[(0, config), (1, source), (2, destination)],
            [
                (count as u32).div_ceil(crate::shared::gpu_constants::FINE_WORKGROUP_SIZE) + 1,
                1,
                1,
            ],
        )?;
    }
    batch.readback(destination)?;
    let routes = Routes::new()?;
    let expected = routes.reference_output(&batch)?;
    assert_eq!(expected.len(), 1);
    let output: Vec<u32> = expected[0]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect();
    assert_eq!(output.len(), initial.len());
    for (index, case) in cases.iter().enumerate() {
        let [a, b, source, destination, coverage, _, rule, _] = *case;
        let scaled = if b == 0 {
            0
        } else if b == 255 {
            source
        } else {
            pack(std::array::from_fn(|lane| {
                multiply(channel(source, lane as u32), b)
            }))
        };
        let alpha = source >> 24;
        let over = if alpha == 0 {
            destination
        } else if alpha == 255 {
            source
        } else {
            pack(std::array::from_fn(|lane| {
                channel(source, lane as u32)
                    + multiply(channel(destination, lane as u32), 255 - alpha)
            }))
        };
        let coverage = f32::from_bits(coverage);
        let filled = if rule == 1 {
            (coverage - 2.0 * (0.5 * coverage).round_ties_even()).abs()
        } else {
            coverage.abs().min(1.0)
        };
        let oracle = [
            multiply(a, b),
            multiply(a, b),
            scaled,
            over,
            source,
            (coverage.clamp(0.0, 1.0) * 255.0 + 0.5) as u32,
            (filled.clamp(0.0, 1.0) * 255.0 + 0.5) as u32,
        ];
        assert_eq!(
            &output[index * 14..index * 14 + 7],
            &oracle,
            "CPU channel/coverage case {index}"
        );
    }
    assert_eq!(&output[count * 14..], &initial[count * 14..]);
    // Floating-point helpers are checked through their final packed pixels,
    // against the actual production WGSL, without a per-backend tolerance.
    routes.check(&batch, &expected, "pixel math exact packed output")?;
    routes.validate()?;
    Ok(())
}
