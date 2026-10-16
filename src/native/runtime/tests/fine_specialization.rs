//! Coarse owns fine specialization; classification must match the emitted stream.
use super::*;
use crate::{Brush, Radius, native::runtime::gpu_support};
use peniko::{Color, kurbo::Rect};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn scene_fine_specialization_membership_survives_work_arena_reuse() -> Result<()> {
    let context = gpu_support::context()?;
    let mut cache = SceneCache::default();
    let mut observed = [false; 5];
    for chunked in [false, true] {
        for count in [0, 1, 257] {
            for mode in [2, 3, 0, 1, 4] {
                let mut canvas = Canvas::new(129, 113, 1.0);
                for i in 0..if mode == 4 { 0 } else { count } {
                    let partial = mode == 1 || mode == 2 && i % 2 == 0;
                    let rect = if partial {
                        Rect::new(3.25, 7.75, 124.5, 107.25)
                    } else {
                        Rect::new(0., 0., 129., 113.)
                    };
                    let color = Color::from_rgba8((i * 37) as u8, 90, 170, 127);
                    canvas.push_rect(
                        rect,
                        Radius::all(if partial { 19.5 } else { 0. }),
                        if mode == 3 {
                            Brush::four_corner(rect, [color; 4])
                        } else {
                            Brush::Solid(color)
                        },
                    );
                }
                let mut batch = gpu_support::batch(&context);
                let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
                let plan = scene.encode_coarse(&mut batch, 0..1, 0..0, chunked, 65535)?;
                batch.readback(scene.work)?;
                let output = context
                    .adapter
                    .submit_compute(&batch)
                    .map_err(|error| format!("{error:?}"))?
                    .readback()?;
                let words: &[u32] = bytemuck::cast_slice(&output[0]);
                for tile in 0..scene.lengths.tile_count {
                    // Descriptor low three bits are the kind; the rest is an
                    // independently proved occluded-prefix count.
                    let kind = words[plan.config().fine_tile_kind_base as usize + tile] & 7;
                    observed[kind as usize] = true;
                    let start = words[tile * 6 + 1] as usize;
                    let end = words[tile * 6 + 2] as usize;
                    for particle in start..end {
                        let tag = words[scene.lengths.tile_count * 6 + particle * 6];
                        if tag == 0 {
                            break;
                        }
                        assert!(
                            match kind {
                                0 => true,
                                1 => false,
                                2 => tag == 2,
                                3 | 4 => matches!(tag, 2 | 9 | 13),
                                _ => unreachable!("coarse kind is defined"),
                            },
                            "chunked={chunked} count={count} mode={mode} tile={tile} kind={kind} tag={tag}"
                        );
                    }
                }
            }
        }
    }
    assert!(observed.into_iter().all(|seen| seen));
    context.check_validation()?;
    Ok(())
}
