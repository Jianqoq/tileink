#[repr(C, align(16))]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Params {
    pub count: u32,
    pub source_offset: u32,
    pub destination_offset: u32,
    pub stride: u32,
    pub value: [u32; 4],
}

pub struct Case {
    pub entry: &'static str,
    pub params: Params,
    pub source: Vec<u8>,
    pub destination: Vec<u8>,
    pub expected: Vec<u8>,
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
                entry,
                params,
                source,
                destination,
                expected,
            });
        }
    }
    cases.extend(sampling_cases());
    cases
}

fn sampling_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for width in [1u32, 17] {
        for (origin, step) in [(-1.5f32, 0.5f32), (16.75, -0.25)] {
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
                    // Independent f64 reference; all coordinates and weights are dyadic,
                    // so these boundary/rounding cases are exactly representable in f32.
                    let x = origin as f64 + id as f64 * step as f64;
                    let fraction = x - x.floor();
                    let left = x.floor().clamp(0.0, (width - 1) as f64) as usize;
                    let right = (x.floor() + 1.0).clamp(0.0, (width - 1) as f64) as usize;
                    for lane in 0..4 {
                        let a = source[4 + left * 4 + lane] as f64;
                        let b = source[4 + right * 4 + lane] as f64;
                        expected[8 + id * 4 + lane] =
                            (a * (1.0 - fraction) + b * fraction + 0.5).floor() as u8;
                    }
                }
                cases.push(Case {
                    entry: "sample_words",
                    params,
                    source,
                    destination,
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
