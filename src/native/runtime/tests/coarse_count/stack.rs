//! Coarse count and every emitter share one validated stack and immutable geometry.
use super::*;

fn mixed_stack(case: usize, entry: &str) -> CountScene {
    // Full path, partial path, full SDF, empty SDF, empty path, then an
    // EvenOdd path with negative winding. Opacity/blend always retain wrappers.
    let mut scene = super::emit_scene::paint_scene(4, false);
    scene.reserve_streams(32, 8);
    let mut clip = scene.draws[..31].to_vec();
    clip[0] = u32::MAX;
    clip[2] = scene.sdf.len() as u32;
    clip[3] = 9;
    clip[6] = u32::MAX;
    clip[8] = 1;
    clip[18] = 0;
    scene.sdf.extend([
        1,
        0,
        0,
        16f32.to_bits(),
        16f32.to_bits(),
        if case == 2 { 0 } else { 4f32.to_bits() },
        0,
        0,
        0,
    ]);
    if case == 3 {
        clip[12..14].fill(0);
        let start = clip[2] as usize;
        scene.sdf[start + 3..start + 5].fill(0);
    }
    scene.draws.extend(clip);
    scene.draws[2 * 31 + 8] = 2;
    scene.draws[3 * 31 + 8] = 3;
    scene.config[5] = 1;
    scene.config[6] = 5;
    // The leading sentinel is outside the active stack range.
    scene.layers = vec![99, 99, 99, 0, 6, 0, 1, 2, 128, 2, 3, 17, 0, 8, 0];
    if case == 1 {
        scene.ranges[1] = 1;
    }
    if case == 4 {
        scene.paths[8] = 0;
        scene.draws[6 * 31 + 12..6 * 31 + 14].fill(0);
    }
    if case == 5 {
        scene.backdrops[0] = u32::MAX;
        scene.draws[6 * 31 + 9] = 1;
    }
    let rejected = matches!(case, 3 | 4);
    let winding = scene.backdrops[0];
    let end = scene.ranges[1];
    let mut stream = Vec::new();
    if case == 1 {
        stream.push([3, winding, 0, 0, end, 0]);
    }
    stream.extend([[5, winding, 0, 0, end, 128], [7, winding, 0, 0, end, 17]]);
    if case != 2 {
        stream.push([12, 0, 0, 0, 0, 8]);
    }
    stream.push([9, 0, 0, 0, 0, 0]);
    if case != 2 {
        stream.push([4, 0, 0, 0, 0, 0]);
    }
    stream.extend([[8, 0, 0, 0, 0, 0], [6, 0, 0, 0, 0, 0]]);
    if case == 1 {
        stream.push([4, 0, 0, 0, 0, 0]);
    }
    stream.push([0; 6]);
    // Scalar/parallel emission also sees a reused, nonempty tile range when
    // coverage rejects the tile. The chunk chain starts from its counted range.
    let count = if rejected && entry == "coarse_emit_chunks" {
        0
    } else {
        stream.len() as u32
    };
    scene.work[..6].copy_from_slice(&[count, 1, 1 + count, 0, 1, 1]);
    scene.expected = scene.work.clone();
    if rejected {
        if entry != "coarse_emit_chunks" {
            scene.expected[12..18].fill(0);
        }
    } else {
        for (index, particle) in stream.iter().enumerate() {
            scene.expected[12 + index * 6..18 + index * 6].copy_from_slice(particle);
        }
    }
    match entry {
        "coarse_emit" => scene.expected[scene.kind_base] = 0,
        "coarse_emit_bins" => scene.expected[scene.kind_base] = u32::from(rejected),
        "coarse_emit_chunks" => {
            let base = scene.kind_base - scene.config[13] as usize * 7;
            scene.expected[base + 2..base + 7].copy_from_slice(&[
                u32::from(!rejected),
                0,
                0,
                0,
                if rejected { 0 } else { 2 },
            ]);
        }
        _ => unreachable!(),
    }
    scene
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_mixed_stacks_keep_matching_counts_and_wrapper_order() -> Result<()> {
    let routes = Routes::new()?;
    for case in 0..6 {
        let mut counted = mixed_stack(case, "coarse_emit");
        counted.expect_counts(
            if matches!(case, 3 | 4) {
                0
            } else {
                counted.work[0]
            },
            0,
        );
        for entry in ["coarse_count", "coarse_count_bins"] {
            routes.check(
                &counted.batch(entry)?,
                &[bytes(&counted.expected)],
                &format!("{entry}, mixed stack case {case}"),
            )?;
        }
        for entry in ["coarse_emit", "coarse_emit_bins", "coarse_emit_chunks"] {
            let scene = mixed_stack(case, entry);
            let batch = if entry == "coarse_emit_chunks" {
                scene.emit_chunks_batch()?
            } else {
                scene.emit_batch(entry)?
            };
            routes.check(
                &batch,
                &[bytes(&scene.expected)],
                &format!("{entry}, mixed stack case {case}"),
            )?;
        }
    }
    routes.validate()
}
