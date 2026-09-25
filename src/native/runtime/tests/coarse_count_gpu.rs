use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

#[path = "coarse_count/scene.rs"]
mod scene;
use super::coarse_routes::Routes;
use scene::{CountScene, bytes, count_scene};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_count_preserves_paged_draw_and_stack_semantics() -> Result<()> {
    let routes = Routes::new()?;
    for count in [0, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..4 {
                for entry in ["coarse_count", "coarse_count_bins"] {
                    let scene = count_scene(count, linked, stack);
                    let batch = scene.batch(entry)?;
                    let expected = [bytes(&scene.expected)];
                    for repetition in 0..3 {
                        routes.check(&batch, &expected, &format!("{entry} count {count} linked {linked} stack {stack} repetition {repetition}"))?;
                    }
                }
            }
        }
    }
    routes.validate()?;

    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_count_checks_glyph_bounds_and_analytic_clip_proofs() -> Result<()> {
    let routes = Routes::new()?;
    let mut scenes = Vec::new();
    // Bitmap glyph cases: translated, boundary-touching, mirrored, empty and invalid images.
    for mode in 0..8 {
        let mut scene = count_scene(1, false, 0);
        scene.config[10] = 1;
        scene.config[11] = 1;
        scene.config[15] = 1;
        scene.draws[1] = 0;
        scene.draws[2] = u32::MAX;
        scene.text = vec![0, 1, 0, 0, 0, 0, 0, 8, 8, 0, 0];
        match mode {
            1 => scene.draws[23] = 16f32.to_bits(),
            2 => scene.draws[23] = (-7.5f32).to_bits(),
            3 => {
                scene.draws[19] = (-1f32).to_bits();
                scene.draws[23] = 8f32.to_bits();
            }
            4 => scene.text[7] = 0,
            5 => scene.text[2] = u32::MAX,
            6 => scene.draws[8] = 1,
            7 => scene.config[15] = 0,
            _ => (),
        }
        let hit = matches!(mode, 0 | 2 | 3 | 7);
        scene.expect_counts(if hit { 2 } else { 0 }, u32::from(hit && mode != 7));
        scenes.push(scene);
    }
    // A clip covering the entire tile is elided only for a translation-only
    // analytic rectangle whose coverage ramp and rounded corners prove full alpha.
    for mode in 0..8 {
        let mut scene = count_scene(1, false, 1);
        let clip = 2 * 31;
        scene.draws[clip + 2] = 0;
        scene.draws[clip + 3] = 9;
        scene.sdf = [1f32, 0., 0., 16., 16., 0., 0., 0., 0.]
            .map(f32::to_bits)
            .to_vec();
        scene.sdf[0] = 1;
        match mode {
            1 => scene.sdf[5] = 4f32.to_bits(),
            2 => scene.draws[clip + 19] = 2f32.to_bits(),
            3 => scene.draws[clip + 4] = 0,
            4 => scene.draws[clip + 3] = 8,
            5 => {
                scene.draws[clip + 12] = 0;
                scene.draws[clip + 13] = 0;
                scene.sdf[3] = 0f32.to_bits();
            }
            6 => {
                scene.draws[clip + 23] = (-1f32).to_bits();
                scene.sdf[1] = 1f32.to_bits();
                scene.sdf[3] = 17f32.to_bits();
            }
            7 => scene.sdf[0] = 2,
            _ => (),
        }
        scene.expect_counts(
            if mode == 5 {
                0
            } else if matches!(mode, 0 | 6) {
                2
            } else {
                4
            },
            0,
        );
        scenes.push(scene);
    }
    for (case, scene) in scenes.iter().enumerate() {
        for entry in ["coarse_count", "coarse_count_bins"] {
            for repetition in 0..3 {
                let batch = scene.batch(entry)?;
                routes.check(
                    &batch,
                    &[bytes(&scene.expected)],
                    &format!("{entry} case {case} repetition {repetition}"),
                )?;
            }
        }
    }
    routes.validate()?;

    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_tile_counts_reduce_chunks_and_validate_wrappers() -> Result<()> {
    let routes = Routes::new()?;
    for chunks in [0u32, 1, 2, 255, 256, 257] {
        for stack in 0..4 {
            let mut scene = count_scene(chunks * COARSE_WORKGROUP_SIZE, false, stack);
            let base = scene.kind_base - scene.config[13] as usize * 7;
            let mut particles = 0u32;
            let mut glyphs = 0u32;
            for chunk in 0..chunks as usize {
                let count = [u32::MAX, 2, 0, 7][chunk % 4];
                scene.work[base + chunk * 7 + 2] = count;
                scene.work[base + chunk * 7 + 4] = chunk as u32;
                particles = particles.wrapping_add(count);
                glyphs = glyphs.wrapping_add(chunk as u32);
            }
            if particles > 0 {
                if stack == 3 {
                    particles = 0;
                    glyphs = 0;
                } else {
                    particles = particles.wrapping_add(if stack == 2 { 3 } else { 1 });
                }
            }
            scene.expect_counts(particles, glyphs);
            let batch = scene.tile_counts_batch()?;
            routes.check(
                &batch,
                &[bytes(&scene.expected)],
                &format!("chunks {chunks} stack {stack}"),
            )?;
        }
    }
    routes.validate()?;

    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_particle_counts_ignore_stale_capacity_and_padded_groups() -> Result<()> {
    let routes = Routes::new()?;
    for draws in [0u32, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..4 {
                let mut scene = count_scene(draws, linked, stack);
                let base = scene.kind_base - scene.config[13] as usize * 7;
                let chunks = draws.div_ceil(COARSE_WORKGROUP_SIZE);
                for chunk in chunks..chunks + 2 {
                    scene.work[base + chunk as usize * 7] = 0;
                    scene.work[base + chunk as usize * 7 + 1] = 0;
                }
                scene.expected = scene.work.clone();
                for chunk in 0..chunks {
                    let start = chunk * COARSE_WORKGROUP_SIZE;
                    let end = (start + COARSE_WORKGROUP_SIZE).min(draws);
                    let count = if stack == 3 {
                        0
                    } else {
                        (start..end)
                            .filter(|i| matches!(i % 8, 0 | 1 | 2 | 3 | 6))
                            .count() as u32
                    };
                    scene.expected[base + chunk as usize * 7 + 2] = count;
                    scene.expected[base + chunk as usize * 7 + 4] = 0;
                }
                let batch = scene.particle_counts_batch()?;
                routes.check(
                    &batch,
                    &[bytes(&scene.expected)],
                    &format!("draws {draws} linked {linked} stack {stack}"),
                )?;
            }
        }
    }
    routes.validate()?;
    Ok(())
}

#[path = "coarse_count/grid.rs"]
mod grid;

#[path = "coarse_count/kinds.rs"]
mod kinds;

#[path = "coarse_count/emit.rs"]
mod emit;

#[path = "coarse_count/emit_chunks.rs"]
mod emit_chunks;

#[path = "coarse_count/emit_scene.rs"]
mod emit_scene;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_invalid_optional_references_leave_streams_empty() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..4 {
        let mut scene = count_scene(1, false, 0);
        scene.draws[2] = u32::MAX;
        match mode {
            0 => scene.draws[0] = 1, // one-past-end path
            1 => scene.draws[0] = u32::MAX,
            2 | 3 => {
                let list = (scene.config[0] * 8 + scene.config[7] * 6 + scene.config[8]) as usize;
                scene.work[list] = if mode == 2 { 8 } else { u32::MAX };
            }
            _ => unreachable!(),
        }
        scene.expect_counts(0, 0);
        for entry in ["coarse_count", "coarse_count_bins"] {
            routes.check(
                &scene.batch(entry)?,
                &[bytes(&scene.expected)],
                &format!("{entry} invalid optional reference {mode}"),
            )?;
        }
    }
    routes.validate()
}
