//! Opaque full-tile particles may discard earlier raster work, while their
//! allocation, glyph destinations, painter order and layer inputs remain intact.
use super::emit_scene::paint_scene;
use super::*;

fn opaque_scene(count: u32, linked: bool, stack: u32, alpha: u32, bins: bool) -> CountScene {
    let mut scene = count_scene(count, linked, stack);
    scene.reserve_streams(count + 4, 4);
    let paint = paint_scene(3, false);
    scene.config[14] = 12;
    scene.sdf = paint.sdf[..12].to_vec();
    let mut group = scene.draws[2 * 31..3 * 31].to_vec();
    scene.draws.clear();
    scene.batches = vec![42; count.max(3) as usize];
    let hidden = count / 2;
    for index in 0..count.max(3) {
        let mut draw = paint.draws[..31].to_vec();
        draw[6] = index * 21;
        scene.draws.extend(draw);
        let mut brush = vec![0; 21];
        brush[0] = 1;
        brush[4] = (if index == hidden { alpha } else { 119 }) << 24 | 0x224466;
        scene.sdf.extend(brush);
    }
    if stack != 0 {
        // Stack geometry has its own path record; painted SDF draws cannot be
        // used as opacity boundaries by the typed scene producer.
        group[8] = if stack == 1 { 1 } else { 2 };
        scene.layers[1] = count.max(3);
        scene.draws.extend(group);
    }
    let list = 6 + scene.config[7] as usize * 6 + scene.config[8] as usize + 2;
    for index in 0..count as usize {
        let at = if linked {
            index / 256 * 257 + 1 + index % 256
        } else {
            index
        };
        scene.work[list + at] = index as u32;
    }
    let wrappers = u32::from(stack == 2);
    let length = if count == 0 {
        0
    } else {
        count + 2 * wrappers + 1
    };
    scene.work[..6].copy_from_slice(&[length, 1, 1 + length, 0, 1, 1]);
    scene.expected = scene.work.clone();
    let mut stream = Vec::new();
    if count != 0 {
        if wrappers != 0 {
            stream.push([5, 1, 0, 0, 0, 0]);
        }
        for index in 0..count {
            stream.push([
                2,
                0,
                0,
                0,
                0,
                (if index == hidden { alpha } else { 119 }) << 24 | 0x224466,
            ]);
        }
        if wrappers != 0 {
            stream.push([6, 0, 0, 0, 0, 0]);
        }
        stream.push([0; 6]);
    }
    for (index, particle) in stream.iter().enumerate() {
        scene.expected[12 + index * 6..18 + index * 6].copy_from_slice(particle);
    }
    scene.expected[scene.kind_base] = if bins {
        if count == 0 {
            1
        } else if wrappers != 0 {
            0
        } else {
            2
        }
    } else {
        u32::from(count == 0)
    };
    if count > 1 && alpha == 255 && wrappers == 0 {
        scene.expected[scene.kind_base] |= hidden << 3;
    }
    scene
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_preserves_allocations_pages_and_layer_semantics() -> Result<()> {
    let routes = Routes::new()?;
    for count in [0, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..3 {
                for alpha in [0, 254, 255] {
                    for entry in ["coarse_emit", "coarse_emit_bins"] {
                        let scene =
                            opaque_scene(count, linked, stack, alpha, entry.ends_with("bins"));
                        routes.check(&scene.emit_batch(entry)?, &[bytes(&scene.expected)],
                            &format!("{entry} occlusion count={count} linked={linked} stack={stack} alpha={alpha}"))?;
                    }
                }
            }
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_chunk_resolution_respects_clip_and_group_depth() -> Result<()> {
    let routes = Routes::new()?;
    let opaque = [2, 0, 0, 0, 0, 0xff224466];
    let translucent = [2, 0, 0, 0, 0, 0x77224466];
    let end = [0; 6];
    for (stream, cutoff) in [
        (vec![translucent, opaque, translucent, end], 2),
        (vec![opaque, translucent, end], 1),
        (vec![translucent, translucent, end], 1),
        (
            vec![
                translucent,
                [3, 0, 0, 0, 0, 0],
                opaque,
                [4, 0, 0, 0, 0, 0],
                end,
            ],
            1,
        ),
        (
            vec![
                translucent,
                [5, 0, 0, 0, 0, 128],
                opaque,
                [6, 0, 0, 0, 0, 0],
                end,
            ],
            1,
        ),
        (
            vec![
                translucent,
                [7, 0, 0, 0, 0, 0],
                opaque,
                [8, 0, 0, 0, 0, 0],
                end,
            ],
            1,
        ),
        (
            vec![[3, 0, 0, 0, 0, 0], opaque, [4, 0, 0, 0, 0, 0], opaque, end],
            4,
        ),
    ] {
        let mut scene = opaque_scene(1, false, 0, 255, false);
        scene.reserve_streams(16, 4);
        scene.work[..6].copy_from_slice(&[
            stream.len() as u32,
            1,
            1 + stream.len() as u32,
            0,
            1,
            1,
        ]);
        for (index, particle) in stream.iter().enumerate() {
            scene.work[12 + index * 6..18 + index * 6].copy_from_slice(particle);
        }
        let references = scene.kind_base - scene.config[13] as usize * 7;
        scene.work[references + 6] = 13; // conservative interpreter classification
        scene.expected = scene.work.clone();
        scene.expected[scene.kind_base] = (cutoff - 1) << 3;
        routes.check(
            &scene.tile_kinds_batch()?,
            &[bytes(&scene.expected)],
            &format!("chunk occlusion cutoff={cutoff} stream={stream:?}"),
        )?;
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_keeps_reused_preallocated_stream_origins() -> Result<()> {
    let routes = Routes::new()?;
    for entry in ["coarse_emit", "coarse_emit_bins"] {
        let mut scene = opaque_scene(257, true, 0, 255, entry.ends_with("bins"));
        scene.expected[1] = 1;
        scene.expected[scene.kind_base] |= 128 << 3;
        routes.check(
            &scene.emit_batch_grid_repeated(entry, [2, 1, 1], 3)?,
            &[bytes(&scene.expected)],
            &format!("{entry} repeated preallocated occlusion"),
        )?;
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_chunk_chain_skips_only_proven_hidden_prefixes() -> Result<()> {
    let routes = Routes::new()?;
    for count in [0, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..3 {
                for alpha in [0, 254, 255] {
                    let mut scene = opaque_scene(count, linked, stack, alpha, true);
                    let base = scene.kind_base - scene.config[13] as usize * 7;
                    for chunk in 0..count.div_ceil(256) {
                        let start = chunk * 256;
                        let len = (count - start).min(256);
                        let opaque = stack != 2
                            && alpha == 255
                            && count / 2 > 0
                            && (start..start + len).contains(&(count / 2));
                        scene.expected
                            [base + chunk as usize * 7 + 2..base + chunk as usize * 7 + 7]
                            .copy_from_slice(&[len, start, 0, 0, 1 | if opaque { 8 } else { 0 }]);
                    }
                    // Spare references are padded capacity, never live work.
                    for chunk in count.div_ceil(256)..scene.config[13] {
                        scene.work[base + chunk as usize * 7..base + chunk as usize * 7 + 2]
                            .fill(0);
                        scene.expected[base + chunk as usize * 7..base + chunk as usize * 7 + 2]
                            .fill(0);
                    }
                    routes.check(&scene.emit_chunks_resolved_batch()?, &[bytes(&scene.expected)],
                        &format!("chunk occlusion count={count} linked={linked} stack={stack} alpha={alpha}"))?;
                }
            }
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_rejects_incomplete_or_truncated_streams() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..5 {
        let mut scene = opaque_scene(3, false, 0, 255, false);
        let refs = scene.kind_base - scene.config[13] as usize * 7;
        scene.work[refs + 6] = 13;
        scene.work[12..18].copy_from_slice(&[2, 0, 0, 0, 0, 0x77224466]);
        scene.work[18..24].copy_from_slice(&[2, 0, 0, 0, 0, 0xff224466]);
        scene.work[24..30].copy_from_slice(&[2, 0, 0, 0, 0, 0x77224466]);
        scene.work[30..36].fill(0);
        match mode {
            0 => scene.work[30] = 9,                  // missing END
            1 => scene.work[2] = scene.config[7] + 1, // capacity truncation
            2 => scene.work[2] = scene.work[1],       // empty interval
            3 => scene.work[24] = 4,                  // unclosed clip contains the opaque color
            _ => scene.work[24] = 5,                  // unmatched begin above the candidate
        }
        scene.expected = scene.work.clone();
        scene.expected[scene.kind_base] = 0;
        routes.check(
            &scene.tile_kinds_batch()?,
            &[bytes(&scene.expected)],
            &format!("incomplete stream {mode}"),
        )?;
    }
    routes.validate()
}
