use super::emit_scene::*;
use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_emit_preserves_order_wrappers_and_capacity_guards() -> Result<()> {
    let routes = Routes::new()?;
    for draws in [0, 1, 255, 256, 257, 513] {
        for linked in [false, true] {
            for stack in 0..4 {
                for capacity in [0, 3, draws + 8] {
                    for entry in ["coarse_emit", "coarse_emit_bins"] {
                        let scene =
                            emission_scene(draws, linked, stack, capacity, entry.ends_with("bins"));
                        routes.check(&scene.emit_batch(entry)?, &[bytes(&scene.expected)], &format!("{entry} draws {draws} linked {linked} stack {stack} capacity {capacity}"))?;
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
fn native_routes_coarse_emit_preserves_paint_fast_paths_and_glyph_indices() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..21 {
        for entry in ["coarse_emit", "coarse_emit_bins"] {
            let scene = paint_scene(mode, entry.ends_with("bins"));
            routes.check(
                &scene.emit_batch(entry)?,
                &[bytes(&scene.expected)],
                &format!("{entry} paint mode {mode}"),
            )?;
        }
    }
    routes.validate()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_emit_reverses_nested_stack_end_order() -> Result<()> {
    let routes = Routes::new()?;
    for entry in ["coarse_emit", "coarse_emit_bins"] {
        let mut scene = paint_scene(4, entry.ends_with("bins"));
        scene.reserve_streams(16, 8);
        scene.config[5] = 1;
        scene.config[6] = 5;
        scene.layers = vec![99, 99, 99, 1, 2, 0x1234, 0, 6, 0, 2, 3, 17, 0, 0, 0];
        let stream = [
            [5, 1, 0, 0, 0, 0x1234],
            [7, 1, 0, 0, 0, 17],
            [12, 0, 0, 0, 0, 0],
            [9, 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0],
            [8, 0, 0, 0, 0, 0],
            [6, 0, 0, 0, 0, 0],
            [0; 6],
        ];
        scene.work[..6].copy_from_slice(&[8, 1, 9, 0, 1, 1]);
        scene.expected = scene.work.clone();
        for (index, record) in stream.iter().enumerate() {
            scene.expected[12 + index * 6..18 + index * 6].copy_from_slice(record);
        }
        if entry.ends_with("bins") {
            scene.expected[scene.kind_base] = 0;
        }
        routes.check(&scene.emit_batch(entry)?, &[bytes(&scene.expected)], entry)?;
    }
    routes.validate()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_emit_carries_glyph_offsets_across_draw_pages() -> Result<()> {
    let routes = Routes::new()?;
    for linked in [false, true] {
        for entry in ["coarse_emit", "coarse_emit_bins"] {
            let scene = glyph_page_scene(linked, entry.ends_with("bins"));
            routes.check(
                &scene.emit_batch(entry)?,
                &[bytes(&scene.expected)],
                &format!("{entry} 257 glyph draws linked {linked}"),
            )?;
        }
    }
    routes.validate()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_emit_preserves_sparse_tiles_and_partial_bin_edges() -> Result<()> {
    let routes = Routes::new()?;
    for mode in 0..3 {
        let scene = emission_grid_scene(mode);
        let batch = if mode == 2 {
            scene.emit_batch_grid("coarse_emit_bins", [5, 1, 1])?
        } else {
            scene.emit_batch_grid("coarse_emit", [scene.config[16] + 1, 1, 1])?
        };
        routes.check(
            &batch,
            &[bytes(&scene.expected)],
            &format!("17x19 emit mode {mode}"),
        )?;
    }
    routes.validate()?;
    Ok(())
}
