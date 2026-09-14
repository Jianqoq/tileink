use super::fine_fixture::{self, FineFixture};
use crate::native::runtime::Result;
use crate::shared::{gpu_coarse::PtclRecord, gpu_constants::FINE_WORKGROUP_SIZE};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_nested_clip_and_group_spills_match_independent_stack_oracles() -> Result<()> {
    let routes = fine_fixture::routes()?;
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    let constants = super::hlsl_constants::read_hlsl("fine/constants.hlsli")?;
    let blend = super::hlsl_constants::read_hlsl("shared/blend/modes.hlsli")?;
    let record = |tag: &str, backdrop: i32, color: u32| PtclRecord {
        tag: tags[tag],
        backdrop,
        color,
        ..Default::default()
    };
    let lanes = FINE_WORKGROUP_SIZE as usize;
    let multiply = |a: u32, b: u32| (a * b + 127) / 255;
    for kind in ["clip", "opacity", "blend"] {
        let local = constants[if kind == "clip" {
            "FINE_LOCAL_CLIP_DEPTH"
        } else {
            "FINE_LOCAL_GROUP_DEPTH"
        }] as usize;
        for depth in [local - 1, local, local + 2] {
            let mut particles = Vec::new();
            let mut writes = Vec::new();
            let expected_color;
            let spill_depth;
            let prefix = if kind == "clip" { 0 } else { 7 };
            let fields = if kind == "clip" {
                1
            } else {
                constants["FINE_GROUP_SPILL_FIELDS"] as usize
            };
            if kind == "clip" {
                spill_depth = (depth + 2).saturating_sub(local) + 1;
                for d in 0..depth + 2 {
                    particles.push(record("PTCL_BEGIN_CLIP", if d == depth { 0 } else { 1 }, 0));
                    if d >= local {
                        writes.push((d - local, vec![if d > depth { 0 } else { 255 }]));
                    }
                }
                particles.push(record("PTCL_COLOR", 0, 0xff13a75b));
                particles.push(record("PTCL_END_CLIP", 0, 0));
                particles.push(record("PTCL_COLOR", 0, 0xff9723cb));
                particles.push(record("PTCL_END_CLIP", 0, 0));
                expected_color = 0x7f291b0d;
                particles.push(record("PTCL_COLOR", 0, expected_color));
                for _ in 0..depth + 1 {
                    particles.push(record("PTCL_END_CLIP", 0, 0));
                }
            } else {
                spill_depth = depth.saturating_sub(local) + 1;
                let begin = if kind == "opacity" {
                    "PTCL_BEGIN_OPACITY"
                } else {
                    "PTCL_BEGIN_BLEND"
                };
                let end = if kind == "opacity" {
                    "PTCL_END_OPACITY"
                } else {
                    "PTCL_END_BLEND"
                };
                let payload = if kind == "opacity" {
                    128
                } else {
                    blend["MIX_MULTIPLY"] | blend["COMPOSE_SRC_OVER"] << 8
                };
                let parent = if kind == "opacity" { 0 } else { 0xff3d85c5 };
                for d in 0..depth {
                    if kind == "blend" {
                        particles.push(record("PTCL_COLOR", 0, parent));
                    }
                    particles.push(record(begin, 1, payload));
                    if d >= local {
                        writes.push((d - local, vec![tags[begin], parent, 255, 255, payload]));
                    }
                }
                let foreground = 0xffd94f91;
                particles.push(record("PTCL_COLOR", 0, foreground));
                let mut channels = foreground.to_le_bytes().map(u32::from);
                for _ in 0..depth {
                    particles.push(record(end, 0, 0));
                    for (c, channel) in channels.iter_mut().enumerate() {
                        *channel = multiply(
                            *channel,
                            if kind == "opacity" {
                                128
                            } else {
                                (parent >> (c * 8)) & 255
                            },
                        );
                    }
                }
                particles.push(record(end, 0, 0)); // Empty group pop is a no-op.
                expected_color = u32::from_le_bytes(channels.map(|v| v as u8));
            }
            particles.push(record("PTCL_END", 0, 0));
            let mut scene = FineFixture::tile(&particles);
            if kind == "clip" {
                scene.config.clip_spill_depth = spill_depth as u32;
            } else {
                scene.config.group_spill_depth = spill_depth as u32;
                scene.config.group_spill_base = prefix as u32;
            }
            scene.spills = vec![0x37373737; prefix + spill_depth * lanes * fields + 7];
            let mut expected_spills = scene.spills.clone();
            for (d, values) in writes {
                for lane in 0..lanes {
                    let base = prefix + (d * lanes + lane) * fields;
                    expected_spills[base..base + fields].copy_from_slice(&values);
                }
            }
            let expected = vec![
                expected_color.to_le_bytes().repeat(lanes),
                bytemuck::cast_slice(&expected_spills).to_vec(),
            ];
            let batch = scene.batch()?;
            for variant in super::reference::FineVariant::ALL {
                routes.check_fine(
                    &batch,
                    &expected,
                    &format!("fine {kind} depth {depth} {variant:?}"),
                    variant,
                )?;
            }
        }
    }
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len());
    routes.validate()
}
