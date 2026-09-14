use super::four_api::Routes;
use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::gpu_constants::FINE_WORKGROUP_SIZE;

// Production text combines integer LCD masking with perceptual coverage curves.
// Check integer semantics independently and compare packed floating results exactly.
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_text_masks_preserve_channel_semantics_and_exact_perceptual_blending() -> Result<()> {
    let routes = Routes::new()?;
    let mut cases = vec![
        [0xff000000, 0xffffffff, 0x808080, 255],
        [0x7f173b59, 0x9f7d1357, 0x4080c0, 127],
    ];
    let colors = [
        0u32, 0xffffffff, 0xff000000, 0xff2040d0, 0xffd04020, 0x7f176b43, 0x01010001, 0xfe81a30b,
    ];
    for dst in colors {
        for src in colors {
            for mask in [0, 0xffffff, 0x010203, 0x7f8081, 0xfefdfc, 0x00ff7f] {
                for clip in [0, 1, 127, 254, 255] {
                    cases.push([dst, src, mask, clip]);
                }
            }
        }
    }
    let mut seed = 7u32;
    for _ in 0..4096 {
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        let mut color = || {
            let a = next() >> 24;
            let rgb = next();
            (a << 24)
                | ((rgb & 255) % (a + 1))
                | (((rgb >> 8) & 255) % (a + 1)) << 8
                | (((rgb >> 16) & 255) % (a + 1)) << 16
        };
        let dst = color();
        let src = color();
        cases.push([dst, src, next() & 0xffffff, next() >> 24]);
    }
    for records in [&cases[..], &cases[..0]] {
        let mut batch = ComputeBatch::new();
        let input = batch.buffer(if records.is_empty() {
            vec![0; 4]
        } else {
            records
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect()
        })?;
        let config = batch.buffer(
            [records.len() as u32, 0, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        )?;
        let output = batch.buffer(vec![0x37; (records.len() * 5 + 7) * 4])?;
        // SAFETY: complete four-word requests, five-word outputs, plus a poisoned
        // output tail. Explicit count keeps padded storage outside the logical domain.
        unsafe {
            batch.dispatch(
                "text_words",
                &[(9, input), (10, output), (11, config)],
                [
                    (records.len() as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1,
                    1,
                    1,
                ],
            )?;
        }
        batch.readback(output)?;
        let expected = routes.reference_output(&batch)?;
        let words: Vec<_> = expected[0]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect();
        let mul = |a: u32, b: u32| (a * b + 127) / 255;
        for (i, &[dst, src, mask, clip]) in records.iter().enumerate() {
            let coverage: [u32; 3] = std::array::from_fn(|c| mul((mask >> (c * 8)) & 255, clip));
            let alpha = coverage.map(|c| mul(src >> 24, c));
            let max_alpha = *alpha.iter().max().unwrap();
            let mut oracle = (max_alpha + mul(dst >> 24, 255 - max_alpha)) << 24;
            for c in 0..3 {
                oracle |= (mul((src >> (c * 8)) & 255, coverage[c])
                    + mul((dst >> (c * 8)) & 255, 255 - alpha[c]))
                    << (c * 8);
            }
            assert_eq!(words[i * 5], oracle, "integer LCD case {i}");
            if i < 2 {
                assert_eq!(words[i * 5 + 1], linear_oracle(dst, src, [coverage[0]; 3]));
                assert_eq!(words[i * 5 + 3], linear_oracle(dst, src, coverage));
            }
            if i == 0 {
                assert_eq!(words[0], 0xff808080);
                assert_eq!(words[1], 0xffbcbcbc);
                assert_eq!(words[3], 0xffbcbcbc);
            }
            if src >> 24 == 0 || clip == 0 {
                assert_eq!(&words[i * 5..i * 5 + 5], &[dst; 5], "no-op case {i}");
            }
        }
        assert_eq!(&words[records.len() * 5..], &[0x37373737; 7]);
        routes.check(
            &batch,
            &expected,
            "text coverage and packed linear blending",
        )?;
    }
    routes.validate()
}

// Independent f64 sRGB transfer and premultiplied over for stable semantic cases.
fn linear_oracle(dst: u32, src: u32, coverage: [u32; 3]) -> u32 {
    let decode = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    let da = f64::from(dst >> 24) / 255.0;
    let sa = f64::from(src >> 24) / 255.0;
    let masks = coverage.map(|v| f64::from(v) / 255.0);
    let a = sa * masks.into_iter().fold(0.0, f64::max);
    let oa = a + da * (1.0 - a);
    let mut packed = ((oa * 255.0).round() as u32) << 24;
    for (c, m) in masks.into_iter().enumerate() {
        let sc = decode(f64::from((src >> (c * 8)) & 255) / (sa * 255.0)) * sa;
        let dc = decode(f64::from((dst >> (c * 8)) & 255) / (da * 255.0)) * da;
        let linear = sc * m + dc * (1.0 - sa * m);
        packed |= ((encode(linear / oa) * oa * 255.0).round() as u32) << (c * 8);
    }
    packed
}
