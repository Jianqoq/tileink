use super::*;

#[test]
fn retained_queries_use_current_order_in_both_storage_modes() {
    let mut canvas = Canvas::new(64, 32, 1.0);
    for rect in [
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Rect::new(-16.0, 0.0, 0.0, 16.0),
        Rect::new(16.0, 0.0, 32.0, 16.0),
        Rect::new(16.0, 0.0, 32.0, 16.0),
    ] {
        canvas.push_rect(rect, crate::Radius::ZERO, Color::WHITE);
    }
    let key = |rank| PainterKey {
        path: std::rc::Rc::from([rank]),
        local: 0,
    };
    let keys = [key(20), key(30), key(10), PainterKey::inactive()];
    let old_order = [0, 3];
    for persistent in [false, true] {
        let mut bins = TileDrawBins::default();
        if persistent {
            bins.reset(&canvas.draw_records, &old_order, Some(&keys), (4, 2));
        } else {
            bins.reset_transient(
                &canvas.draw_records,
                &old_order,
                Some(&keys),
                (4, 2),
                &mut Vec::new(),
                false,
            );
        }
        assert_eq!(
            bins.draws_in_bounds(Bounds::new(16, 0, 32, 16), &old_order, Some(&keys)),
            [2]
        );
        assert_eq!(
            bins.draws_in_bounds(Bounds::new(-16, 0, 0, 16), &old_order, Some(&keys)),
            [2, 0, 1]
        );
        assert!(
            bins.draws_in_bounds(Bounds::new(32, 16, 48, 32), &old_order, Some(&keys))
                .is_empty()
        );
    }
}
