use super::*;

#[test]
fn persistent_group_reorder_matches_exact_pixels_across_order_rebalancing() {
    if !run_wgpu_tests() {
        return;
    }
    for nested in [false, true] {
        let root = RetainedNodeId::for_owner(82_100);
        let empty = RetainedNodeId::for_owner(82_101);
        let group = RetainedNodeId::for_owner(82_102);
        let first = RetainedNodeId::for_owner(82_103);
        let second = RetainedNodeId::for_owner(82_104);
        let solid = |rect, color| {
            let mut canvas = Canvas::new(35, 19, 1.0);
            canvas.push_rect(rect, crate::Radius::ZERO, color);
            std::rc::Rc::new(canvas)
        };
        let mut scene = RetainedScene::new(35, 19, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        let parent = if nested {
            transaction.insert_group(RetainedParent::content(root), None, group);
            group
        } else {
            root
        };
        transaction.insert_group(RetainedParent::content(parent), None, empty);
        transaction
            .insert_scene(
                RetainedParent::content(parent),
                None,
                first,
                solid(Rect::new(1.0, 1.0, 24.0, 18.0), Color::WHITE),
                Affine::IDENTITY,
            )
            .insert_scene(
                RetainedParent::content(parent),
                None,
                second,
                solid(Rect::new(8.0, 1.0, 34.0, 18.0), Color::BLACK),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        let expected: [Vec<u32>; 2] = std::array::from_fn(|phase| {
            (0..19)
                .flat_map(|y| {
                    (0..35).map(move |x| {
                        if !(1..18).contains(&y) || !(1..34).contains(&x) {
                            u32::from_le_bytes([0, 0, 0, 0])
                        } else if x < 8 || (phase == 1 && x < 24) {
                            u32::from_le_bytes([255, 255, 255, 255])
                        } else {
                            u32::from_le_bytes([0, 0, 0, 255])
                        }
                    })
                })
                .collect()
        });
        let mut renderer = new_test_renderer(35, 19, Color::TRANSPARENT);
        renderer.render_retained(&scene);
        // Exercise both orders through repeated incremental updates and sibling order rebalancing.
        // Full-image assertions also catch stale overlap pixels and damage escaping its bounds.
        for frame in 0..600 {
            let phase = frame % 2;
            let order = if phase == 0 {
                [first, second]
            } else {
                [second, first]
            };
            scene
                .transaction()
                .move_before(order[0], order[1])
                .commit()
                .unwrap();
            renderer.render_retained(&scene);
            assert_eq!(
                renderer.image().pixels,
                expected[phase],
                "nested={nested}, frame={frame}"
            );
        }
    }
}
