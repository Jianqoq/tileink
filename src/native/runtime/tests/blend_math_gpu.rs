use super::four_api::Routes;
use crate::{
    native::runtime::{Result, compute::ComputeBatch},
    shared::gpu_constants::FINE_WORKGROUP_SIZE,
};

fn color(alpha: u32, variant: u32) -> u32 {
    let channels = match variant % 4 {
        0 => [alpha, 0, 0],
        1 => [0, alpha, 0],
        2 => [0, 0, alpha],
        _ => [alpha / 3, alpha / 2, alpha],
    };
    channels[0] | channels[1] << 8 | channels[2] << 16 | alpha << 24
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_blend_math_covers_all_mix_and_compose_modes() -> Result<()> {
    let alphas = [
        0u32, 1, 2, 3, 31, 63, 64, 126, 127, 128, 129, 191, 192, 253, 254, 255,
    ];
    // Color + DestOver exposed a byte difference between the two wgpu APIs.
    let mut records = vec![[color(254, 3), color(64, 0), 14 | (4 << 8), 0]];
    // W3C backdrop endpoints take precedence over the source singularity.
    // Check both generic Copy and SrcIn paths (SrcOver has its own formula).
    records.extend([
        [0xffffffff, 0xff000000, 6 | (1 << 8), 0],
        [0xffffffff, 0xff000000, 6 | (5 << 8), 0],
        [0xff000000, 0xffffffff, 7 | (1 << 8), 0],
        [0xff000000, 0xffffffff, 7 | (5 << 8), 0],
    ]);
    for mix in 0..16u32 {
        for compose in 0..14u32 {
            for source_alpha in alphas {
                for destination_alpha in alphas {
                    for variant in 0..4u32 {
                        records.push([
                            color(source_alpha, variant),
                            color(destination_alpha, variant + 1),
                            mix | (compose << 8),
                            0,
                        ]);
                    }
                }
            }
        }
    }
    // Achromatic, equal-channel, saturated and transparent colors exercise all
    // nonseparable sorting ties and both clip-color correction branches.
    let palette = [
        0u32, 0xff000000, 0x01010101, 0x7f7f7f7f, 0x7f3f3f3f, 0xffff0000, 0xff00ffff, 0xffff00ff,
        0xffffff00, 0xff808080, 0xffffffff,
    ];
    let mut seed = 0x61c88647u32;
    let mut next_color = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let alpha = seed >> 24;
        let channels = [seed & 255, (seed >> 8) & 255, (seed >> 16) & 255]
            .map(|channel| channel * (alpha + 1) / 256);
        channels[0] | channels[1] << 8 | channels[2] << 16 | alpha << 24
    };
    let random_pairs: Vec<_> = (0..768).map(|_| (next_color(), next_color())).collect();
    for mix in 0..16u32 {
        for compose in 0..14u32 {
            for source in palette {
                for destination in palette {
                    records.push([source, destination, mix | (compose << 8), 0]);
                }
            }
            for &(source, destination) in &random_pairs {
                records.push([source, destination, mix | (compose << 8), 0]);
            }
        }
    }
    let count = records.len();
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytes(&[count as u32, 0, 0, 0]))?;
    let source = batch.buffer(bytes(
        &records.iter().flatten().copied().collect::<Vec<_>>(),
    ))?;
    let destination = batch.buffer(bytes(&vec![0xa1b2c3d4; count + 4]))?;
    // SAFETY: one full input and output record per invocation; rounded groups
    // are deliberately padded and must preserve the trailing guard words.
    unsafe {
        batch.dispatch(
            "blend_math_words",
            &[(0, config), (1, source), (2, destination)],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(destination)?;
    let routes = Routes::new()?;
    let expected = routes.reference_output(&batch)?;
    assert_eq!(
        expected[0][2], 181,
        "fixed luminosity FMA order at a half-channel boundary"
    );
    for (index, oracle) in [0xff000000u32, 0xff000000, 0xffffffff, 0xffffffff]
        .into_iter()
        .enumerate()
    {
        let offset = (index + 1) * 4;
        assert_eq!(
            u32::from_le_bytes(expected[0][offset..offset + 4].try_into().unwrap()),
            oracle,
            "Dodge/Burn endpoint precedence case {index}"
        );
    }
    for (i, record) in records.iter().enumerate() {
        if record[2] & 255 == 0 {
            let oracle = match record[2] >> 8 {
                0 => Some(0),
                1 => Some(record[0]),
                2 => Some(record[1]),
                _ => None,
            };
            if let Some(oracle) = oracle {
                assert_eq!(
                    u32::from_le_bytes(expected[0][i * 4..i * 4 + 4].try_into().unwrap()),
                    oracle,
                    "independent composition case {i}"
                );
            }
        }
    }
    assert_eq!(
        &expected[0][count * 4..],
        bytes(&[0xa1b2c3d4; 4]).as_slice()
    );
    routes.check(&batch, &expected, "all blend/compose modes")?;
    routes.validate()
}
