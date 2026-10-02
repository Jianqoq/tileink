use super::*;

fn stroke_scene(mode: u32) -> CountScene {
    let mut scene = count_scene(1, false, 0);
    scene.draws[2] = 0;
    scene.draws[3] = 17;
    scene.sdf = vec![0; 17];
    scene.sdf[0] = 3; // centered rectangle stroke
    scene.sdf[1..5].copy_from_slice(&[-32f32, -32., 48., 48.].map(f32::to_bits));
    scene.sdf[9..13].fill(1f32.to_bits()); // serialized half widths
    match mode {
        1 => scene.sdf[9..13].copy_from_slice(&[3f32, 4., 5., 6.].map(f32::to_bits)),
        2 => scene.sdf[5..9].fill(32f32.to_bits()),
        3 => scene.sdf[1..5].copy_from_slice(&[0f32, 0., 32., 32.].map(f32::to_bits)),
        4 => scene.sdf[9..13].fill(40f32.to_bits()), // collapsed inner rectangle
        5 => scene.draws[19] = 2f32.to_bits(),       // unsupported scale
        6 => scene.draws[4] = 0,                     // shadow alias is not an empty-stroke proof
        7 => scene.draws[3] = 12,                    // incomplete stroke record
        8 => scene.sdf[0] = 1,                       // fills must not be rejected
        9 => {
            scene.sdf[1..5].copy_from_slice(&[-28f32, -24., 52., 56.].map(f32::to_bits));
            scene.draws[23] = (-4f32).to_bits();
            scene.draws[24] = (-8f32).to_bits();
        }
        10 => scene.sdf[9..13].copy_from_slice(&[-1f32, 2., -3., 4.].map(f32::to_bits)),
        11 => scene.sdf[1..5].copy_from_slice(&[-0.25f32, -0.25, 16.25, 16.25].map(f32::to_bits)),
        12 => scene.sdf[5] = f32::NAN.to_bits(),
        13 => {
            scene.sdf[1..5]
                .copy_from_slice(&[1e7f32 - 1., -32., 1e7 + 129., 48.].map(f32::to_bits));
            scene.draws[23] = (-1e7f32).to_bits();
        }
        14 => scene.sdf[9] = f32::INFINITY.to_bits(),
        15 => {
            scene.sdf[1..5].copy_from_slice(&[48f32, 48., -32., -32.].map(f32::to_bits));
        }
        16 => scene.draws[20] = 0.1f32.to_bits(), // unsupported shear
        _ => {}
    }
    scene
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_rect_stroke_interiors_have_no_particles() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..17 {
        let mut scene = stroke_scene(mode);
        let empty = matches!(mode, 0 | 1 | 2 | 9 | 10 | 15);
        scene.expect_counts(if empty { 0 } else { 2 }, 0);
        for entry in ["coarse_count", "coarse_count_bins"] {
            routes.check(
                &scene.batch(entry)?,
                &[bytes(&scene.expected)],
                &format!("{entry} stroke {mode}"),
            )?;
        }
        let base = scene.kind_base - scene.config[13] as usize * 7;
        scene.work[base] = 0;
        scene.work[base + 1] = 0;
        scene.expected = scene.work.clone();
        scene.expected[base + 2] = u32::from(!empty);
        scene.expected[base + 4] = 0;
        routes.check(
            &scene.particle_counts_batch()?,
            &[bytes(&scene.expected)],
            &format!("chunk count stroke {mode}"),
        )?;
    }
    routes.validate()
}

// Glyph routing has priority over optional analytic geometry. A stroke proof
// cannot remove a glyph particle or disagree with the count routes.
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_stroke_proof_preserves_glyph_priority() -> Result<()> {
    let routes = Routes::new()?;
    for alias in [false, true] {
        let mut scene = stroke_scene(0);
        if !alias {
            scene.draws[2] = u32::MAX;
        }
        scene.draws[1] = 0;
        scene.config[10] = 1;
        scene.config[11] = 1;
        scene.config[15] = 1;
        scene.text = vec![0, 1, 0, 0, 0, 0, 0, 8, 8, 0, 0];
        scene.expect_counts(2, 1);
        for entry in ["coarse_count", "coarse_count_bins"] {
            routes.check(&scene.batch(entry)?, &[bytes(&scene.expected)], entry)?;
        }
        scene.reserve_streams(4, 2);
        scene.work[..6].copy_from_slice(&[2, 1, 3, 1, 0, 1]);
        scene.expected = scene.work.clone();
        scene.expected[12..18].copy_from_slice(&[10, 0, 0, 0, 1, 0]);
        scene.expected[18..24].fill(0);
        scene.expected[30] = 0;
        scene.expected[scene.kind_base] = 0;
        for entry in ["coarse_emit", "coarse_emit_bins"] {
            routes.check(
                &scene.emit_batch(entry)?,
                &[bytes(&scene.expected)],
                &format!("{entry} glyph stroke alias {alias}"),
            )?;
        }
    }
    routes.validate()
}
