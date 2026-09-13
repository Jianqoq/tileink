use super::*;

// Non-square grid crosses both bin axes; the empty final tile still carries
// the total live reference offset produced by prefix allocation.
pub(super) fn grid_scene() -> (CountScene, Vec<u32>, usize) {
    let mut scene = count_scene(1, false, 0);
    let tiles = 17usize * 19;
    let counts: Vec<u32> = (0..tiles)
        .map(|tile| {
            if tile + 1 == tiles {
                0
            } else {
                (tile % 3) as u32
            }
        })
        .collect();
    let indices: usize = counts.iter().map(|n| *n as usize).sum();
    let refs = counts.iter().filter(|n| **n != 0).count();
    let draw_base = tiles * 6;
    let index_base = draw_base + tiles * 2;
    let tile_emit_base = index_base + indices;
    let emit_base = tile_emit_base + tiles * 2;
    let kind_base = emit_base + (refs + 2) * 7;
    let active_base = kind_base + tiles + 3;
    let active = [321u32, 16, 289, 1, 17, 0, 322];
    scene.work = vec![0x45454545; active_base + active.len() + 3];
    let mut index = 0;
    let mut reference = 0;
    for (tile, count) in counts.iter().enumerate() {
        scene.work[draw_base + tile * 2] = if *count == 0 {
            u32::MAX
        } else {
            0x80000000 | index as u32
        };
        scene.work[draw_base + tile * 2 + 1] = *count;
        for _ in 0..*count {
            scene.work[index_base + index] = 0;
            index += 1;
        }
        scene.work[tile_emit_base + tile * 2] = u32::from(*count != 0);
        scene.work[tile_emit_base + tile * 2 + 1] = reference as u32;
        if *count != 0 {
            scene.work[emit_base + reference * 7] = tile as u32;
            scene.work[emit_base + reference * 7 + 1] = 0;
            scene.work[emit_base + reference * 7 + 2] = *count;
            scene.work[emit_base + reference * 7 + 4] = 0;
            reference += 1;
        }
    }
    for spare in refs..refs + 2 {
        scene.work[emit_base + spare * 7] = 1;
        scene.work[emit_base + spare * 7 + 1] = 0;
    }
    scene.work[active_base..active_base + active.len()].copy_from_slice(&active);
    scene.config[0] = tiles as u32;
    scene.config[1] = 17;
    scene.config[2] = 19;
    scene.config[7] = 0;
    scene.config[8] = 0;
    scene.config[12] = indices as u32;
    scene.config[13] = refs as u32 + 2;
    scene.config[16] = tiles as u32;
    scene.config[17] = active_base as u32;
    scene.kind_base = kind_base;
    scene.expected = scene.work.clone();
    (scene, counts, emit_base)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_counts_cover_partial_bins_sparse_tiles_and_final_empty_range() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..4 {
        let (mut scene, counts, _) = grid_scene();
        let selected: Vec<usize> = if mode == 1 {
            scene.config[18] = 1;
            scene.config[16] = 7;
            vec![321, 16, 289, 1, 17, 0, 322]
        } else {
            (0..counts.len()).collect()
        };
        for tile in selected {
            scene.expected[tile * 6] = if counts[tile] == 0 {
                0
            } else {
                counts[tile] + 1
            };
            scene.expected[tile * 6 + 3] = 0;
            scene.expected[scene.kind_base + tile] = u32::from(counts[tile] == 0);
        }
        let batch = match mode {
            0 | 1 => scene.batch_grid("coarse_count", [scene.config[16] + 1, 1, 1])?,
            2 => scene.batch_grid("coarse_count_bins", [5, 1, 1])?,
            _ => scene.tile_counts_batch()?,
        };
        routes.check(
            &batch,
            &[bytes(&scene.expected)],
            &format!("17x19 tile counts mode {mode}"),
        )?;
    }
    let (mut scene, counts, base) = grid_scene();
    for (reference, count) in counts.into_iter().filter(|n| *n != 0).enumerate() {
        scene.work[base + reference * 7 + 2] = 0xedededed;
        scene.work[base + reference * 7 + 4] = 0xedededed;
        scene.expected[base + reference * 7 + 2] = count;
        scene.expected[base + reference * 7 + 4] = 0;
    }
    let batch = scene.particle_counts_batch()?;
    routes.check(
        &batch,
        &[bytes(&scene.expected)],
        "17x19 chunk counts; empty final tile; stale padded group",
    )?;
    routes.validate()?;
    Ok(())
}
