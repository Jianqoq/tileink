use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_routes_coarse_tile_kinds_reduce_flags_and_respect_stack_precedence() -> Result<()> {
    let routes = Routes::new()?;
    for left in [0u32, 1, 2, 3, 4, 5, 6, 7, 128] {
        for right in [0u32, 1, 2, 3, 4, 128] {
            for stack in 0..4 {
                for empty in [false, true] {
                    let mut scene = count_scene(512, false, stack);
                    let base = scene.kind_base - scene.config[13] as usize * 7;
                    scene.work[0] = u32::from(!empty);
                    scene.work[base + 6] = left;
                    scene.work[base + 7 + 6] = right;
                    let flags = left | right;
                    // Independent truth table: wrapper/invalid stack forces interpreter;
                    // OTHER takes precedence over mixed/SDF/color, unknown bits are ignored.
                    let expected = if empty {
                        1
                    } else if stack >= 2 || flags & 4 != 0 {
                        0
                    } else {
                        [1, 2, 3, 4][(flags & 3) as usize]
                    };
                    scene.expected = scene.work.clone();
                    scene.expected[scene.kind_base] = expected;
                    routes.check(
                        &scene.tile_kinds_batch()?,
                        &[bytes(&scene.expected)],
                        &format!("flags {left}|{right} stack {stack} empty {empty}"),
                    )?;
                }
            }
        }
    }
    routes.validate()?;
    Ok(())
}
