use crate::native::runtime::program::{Dispatch, Params};

pub struct Case {
    pub dispatch: Dispatch,
    pub expected: Vec<u8>,
}
impl std::ops::Deref for Case {
    type Target = Dispatch;
    fn deref(&self) -> &Dispatch {
        &self.dispatch
    }
}

pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for entry in ["clear_words", "copy_words", "layout_words"] {
        for count in [0, 1, 63, 64, 65, 129] {
            let params = Params {
                count,
                source_offset: 4,
                destination_offset: 8,
                stride: 32,
                value: [0xff804020, u32::MAX, 0x12345678, 0xdeadbeef],
            };
            let source: Vec<u8> = (0..count + 4)
                .flat_map(|i| (i.wrapping_mul(0x9e3779b9) ^ 0x55aa55aa).to_le_bytes())
                .collect();
            let destination = vec![0xa5; (count as usize + 1) * 32];
            let mut expected = destination.clone();
            for id in 0..count {
                let offset = 8 + id as usize * if entry == "layout_words" { 32 } else { 4 };
                match entry {
                    "clear_words" => {
                        expected[offset..offset + 4].copy_from_slice(&params.value[0].to_le_bytes())
                    }
                    "copy_words" => expected[offset..offset + 4]
                        .copy_from_slice(&source[4 + id as usize * 4..8 + id as usize * 4]),
                    _ => {
                        for (lane, add) in [id, params.source_offset, params.stride, count]
                            .into_iter()
                            .enumerate()
                        {
                            expected[offset + lane * 4..offset + lane * 4 + 4].copy_from_slice(
                                &params.value[lane].wrapping_add(add).to_le_bytes(),
                            );
                        }
                    }
                }
            }
            cases.push(Case {
                dispatch: Dispatch {
                    entry,
                    params,
                    source,
                    destination,
                },
                expected,
            });
        }
    }
    cases.extend(sampling_cases());
    let texture_cases: Vec<_> = cases
        .iter()
        .filter(|c| c.entry == "sample_words")
        .map(|case| {
            let mut dispatch = case.dispatch.clone();
            dispatch.params.value[3] = 1;
            Case {
                dispatch,
                expected: case.expected.clone(),
            }
        })
        .collect();
    cases.extend(texture_cases);
    cases
}

fn sampling_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for width in [1u32, 17, 256] {
        for (origin, step) in [
            (-1.5f32, 0.5f32),
            (16.75, -0.25),
            (f32::from_bits(0x3effffff), 0.0),
            (0.5, 0.0),
            (f32::from_bits(0x3f000001), 0.0),
            (64.0, -0.5),
            (-0.0, f32::MIN_POSITIVE),
            (0.1, 0.1),
        ] {
            for count in [0, 1, 63, 64, 65, 129] {
                let params = Params {
                    count,
                    source_offset: 4,
                    destination_offset: 8,
                    stride: 4,
                    value: [origin.to_bits(), step.to_bits(), width, 0],
                };
                let source: Vec<u8> = (0..width + 2)
                    .flat_map(|i| (i.wrapping_mul(0x9e3779b9) ^ 0x55aa55aa).to_le_bytes())
                    .collect();
                let destination = vec![0xa5; (count as usize + 4) * 4];
                let mut expected = destination.clone();
                for id in 0..count as usize {
                    // Independent f64/i64 oracle; no shader bit-decoding or
                    // integer packed-channel operations are reused here.
                    let x = (origin as f64 * 65536.0).round() as i64
                        + id as i64 * (step as f64 * 65536.0).round() as i64;
                    let base = x.div_euclid(65536);
                    let fraction = x.rem_euclid(65536) as f64 / 65536.0;
                    let left = base.clamp(0, (width - 1) as i64) as usize;
                    let right = (base + 1).clamp(0, (width - 1) as i64) as usize;
                    for lane in 0..4 {
                        let a = source[4 + left * 4 + lane] as f64;
                        let b = source[4 + right * 4 + lane] as f64;
                        expected[8 + id * 4 + lane] =
                            (a * (1.0 - fraction) + b * fraction + 0.5).floor() as u8;
                    }
                }
                cases.push(Case {
                    dispatch: Dispatch {
                        entry: "sample_words",
                        params,
                        source,
                        destination,
                    },
                    expected,
                });
            }
        }
    }
    cases
}

#[test]
fn host_parameters_match_shader_offsets_size_and_alignment() {
    assert_eq!(std::mem::size_of::<Params>(), 32);
    assert_eq!(std::mem::align_of::<Params>(), 16);
    assert_eq!(
        [
            std::mem::offset_of!(Params, count),
            std::mem::offset_of!(Params, source_offset),
            std::mem::offset_of!(Params, destination_offset),
            std::mem::offset_of!(Params, stride),
            std::mem::offset_of!(Params, value)
        ],
        [0, 4, 8, 12, 16]
    );
}
