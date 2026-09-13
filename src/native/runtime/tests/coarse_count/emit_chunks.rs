use super::*;

fn stale_refs(scene: &mut CountScene) -> usize {
    let base = scene.kind_base - scene.config[13] as usize * 7;
    for index in scene.config[13] - 2..scene.config[13] {
        scene.work[base + index as usize * 7] = 0;
        scene.work[base + index as usize * 7 + 1] = 0;
        scene.expected[base + index as usize * 7] = 0;
        scene.expected[base + index as usize * 7 + 1] = 0;
    }
    base
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_emit_chunks_chain_preserves_order_and_ignores_spare_capacity() -> Result<()> {
    let routes = Routes::new()?;
    for draws in [0u32, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..4 {
                let mut scene =
                    super::emit_scene::emission_scene(draws, linked, stack, draws + 8, false);
                let base = stale_refs(&mut scene);
                let mut offset = 0u32;
                for chunk in 0..draws.div_ceil(COARSE_WORKGROUP_SIZE) {
                    let count = if stack == 3 {
                        0
                    } else {
                        (chunk * COARSE_WORKGROUP_SIZE
                            ..((chunk + 1) * COARSE_WORKGROUP_SIZE).min(draws))
                            .filter(|i| matches!(i % 8, 0 | 1 | 2 | 3 | 6))
                            .count() as u32
                    };
                    let at = base + chunk as usize * 7;
                    scene.expected[at + 2] = count;
                    scene.expected[at + 3] = offset;
                    scene.expected[at + 4] = 0;
                    scene.expected[at + 5] = 0;
                    scene.expected[at + 6] = if count == 0 { 0 } else { 4 };
                    offset += count;
                }
                routes.check(
                    &scene.emit_chunks_batch()?,
                    &[bytes(&scene.expected)],
                    &format!("chunk emission draws {draws} linked {linked} stack {stack}"),
                )?;
            }
        }
    }
    routes.validate()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_emit_chunks_classifies_paint_and_keeps_glyph_destinations() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..21 {
        let kind_scene = super::emit_scene::paint_scene(mode, true);
        let kind = kind_scene.expected[kind_scene.kind_base];
        let mut scene = super::emit_scene::paint_scene(mode, false);
        let base = stale_refs(&mut scene);
        scene.expected[base + 2] = 1;
        scene.expected[base + 3] = 0;
        scene.expected[base + 4] = if (14..=16).contains(&mode) { 2 } else { 0 };
        scene.expected[base + 5] = 0;
        scene.expected[base + 6] = [4, 0, 1, 2, 3][kind as usize];
        routes.check(
            &scene.emit_chunks_batch()?,
            &[bytes(&scene.expected)],
            &format!("chunk emission paint {mode}"),
        )?;
    }
    routes.validate()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_emit_chunks_chain_carries_glyphs_and_nonzero_tile_ranges() -> Result<()> {
    let routes = Routes::new()?;
    for linked in [false, true] {
        let mut scene = super::emit_scene::glyph_page_scene(linked, false);
        let base = stale_refs(&mut scene);
        scene.expected[base + 2..base + 7].copy_from_slice(&[256, 0, 512, 0, 4]);
        scene.expected[base + 9..base + 14].copy_from_slice(&[1, 256, 2, 512, 4]);
        routes.check(
            &scene.emit_chunks_batch()?,
            &[bytes(&scene.expected)],
            &format!("chunk glyph carry linked {linked}"),
        )?;
    }
    let mut scene = super::emit_scene::emission_grid_scene(0);
    let base = stale_refs(&mut scene);
    let tile_emit_base = base - scene.config[0] as usize * 2;
    for tile in 0..scene.config[0] as usize {
        if scene.work[tile_emit_base + tile * 2] == 0 {
            continue;
        }
        let reference = scene.work[tile_emit_base + tile * 2 + 1] as usize;
        let count = scene.work[tile * 6] - 1;
        scene.expected[base + reference * 7 + 2..base + reference * 7 + 7]
            .copy_from_slice(&[count, 0, 0, 0, 4]);
    }
    routes.check(
        &scene.emit_chunks_batch()?,
        &[bytes(&scene.expected)],
        "17x19 chunk output; final empty tile; padded groups",
    )?;
    routes.validate()?;
    Ok(())
}
