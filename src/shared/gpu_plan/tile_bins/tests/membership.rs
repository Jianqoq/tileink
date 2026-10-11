use super::*;

#[test]
fn partial_edge_tiles_preserve_membership_across_sparse_and_rank_updates() {
    let size = (33, 17);
    let make = |frame| {
        let mut canvas = Canvas::new(size.0 * crate::TILE_SIZE, size.1 * crate::TILE_SIZE, 1.0);
        for draw in 0..272 {
            let rect = match (draw, frame) {
                (0, _) => Rect::new(0.0, 0.0, 528.0, 272.0),
                (1, 1) => Rect::new(512.0, 256.0, 528.0, 272.0),
                (1, 2) => Rect::new(-32.0, -32.0, -16.0, -16.0),
                (2, 1 | 2) => Rect::new(0.0, 0.0, 16.0, 16.0),
                (271, _) => Rect::new(512.0, 256.0, 528.0, 272.0),
                _ => Rect::new(240.0, 240.0, 288.0, 272.0),
            };
            canvas.push_rect(rect, crate::Radius::ZERO, Color::BLACK);
        }
        canvas
    };
    let initial = make(0);
    let mut bins = build_tile_draw_bins(&initial);
    let mut keys: Vec<_> = (0..272)
        .map(|draw| PainterKey {
            path: std::rc::Rc::from([draw as u128]),
            local: 0,
        })
        .collect();
    for frame in 1..=3 {
        let canvas = make(frame);
        if frame == 3 {
            keys[1].path = std::rc::Rc::from([1000]);
        }
        let mut order: Vec<_> = (0..272).collect();
        order.sort_unstable_by(|&a, &b| keys[a as usize].cmp(&keys[b as usize]));
        assert!(bins.update_changed(
            &canvas.draw_records,
            &order,
            Some(&keys),
            size,
            &[1..3, 2..5, 271..272]
        ));
        for y in 0..size.1 {
            for x in 0..size.0 {
                let expected: Vec<_> = order
                    .iter()
                    .copied()
                    .filter(|&draw| {
                        let b = canvas.draw_records[draw as usize].tile_bbox(size.0, size.1);
                        b.x0 <= x && x < b.x1 && b.y0 <= y && y < b.y1
                    })
                    .collect();
                assert_eq!(
                    bins.tile_draws((y * size.0 + x) as usize),
                    expected,
                    "frame {frame}, tile ({x}, {y})"
                );
            }
        }
        assert_eq!(
            bins.draws_in_bounds(Bounds::new(512, 256, 528, 272), &order),
            order
                .iter()
                .copied()
                .filter(|&id| id == 0 || id == 271 || (id == 1 && frame == 1))
                .collect::<Vec<_>>()
        );
    }
}
