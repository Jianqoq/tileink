use super::super::*;
use super::*;
use crate::shared::bounds::PixelBounds;
use crate::{Bounds, Radius};
use peniko::{Color, kurbo::Rect};

#[test]
fn dense_damage_matches_exact_bin_membership_including_inactive_and_degenerate_draws() {
    let size = (65 * crate::TILE_SIZE + 3, 5 * crate::TILE_SIZE + 7);
    let mut canvas = Canvas::new(size.0, size.1, 1.0);
    for rect in [
        Rect::new(0.0, 0.0, 1043.0, 83.0),
        Rect::new(100.0, 0.0, 116.0, 16.0),
        Rect::new(1041.0, 81.0, 1055.0, 90.0),
        Rect::new(-32.0, -32.0, -16.0, -16.0),
        Rect::new(0.0, 0.0, 1043.0, 83.0),
        Rect::new(0.0, 0.0, 1043.0, 83.0),
        Rect::new(8.0, 8.0, 12.0, 12.0),
    ] {
        canvas.push_rect(rect, Radius::ZERO, Color::BLACK);
    }
    canvas.draw_records[6].pixel_bounds = PixelBounds {
        x0: 8,
        y0: 8,
        x1: 8,
        y1: 8,
    };
    let ids = [7, 3, 11, 13, 17, u32::MAX, 23];
    for keyed in [false, true] {
        let mut plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        if keyed {
            let mut keys = (0..ids.len())
                .map(|draw| crate::canvas::PainterKey {
                    path: std::rc::Rc::from([draw as u128]),
                    local: 0,
                })
                .collect::<Vec<_>>();
            keys[4] = crate::canvas::PainterKey::inactive();
            canvas.persistent_root = Some(crate::RetainedNodeId::for_owner(1));
            canvas.painter.keys = keys;
            canvas.painter.batch_ids = ids.to_vec();
            canvas.painter.batch_counts = vec![0; 24];
            for batch in ids.into_iter().filter(|&batch| batch != u32::MAX) {
                canvas.painter.batch_counts[batch as usize] += 1;
            }
        } else {
            plan.draw_batch_ids = std::rc::Rc::new(ids.to_vec());
        }
        for flat in [false, true] {
            let mut bins = build_tile_draw_bins(&canvas);
            if flat {
                bins.reset_transient(
                    &canvas.draw_records,
                    &plan.draw_order,
                    canvas
                        .retained_painter()
                        .map(|painter| painter.keys.as_slice()),
                    (66, 6),
                    &mut Vec::new(),
                    false,
                );
            }
            let mut query = ActiveBatches::default();
            let full = DamageTiles::full(size);
            let expected = if keyed {
                vec![3, 7, 11, 23]
            } else {
                vec![3, 7, 11, 17, 23]
            };
            assert_eq!(query.collect(&bins, &canvas, &plan, &full), expected);
            let mut holes = DamageTiles::new(size);
            holes.add_bounds(Bounds::new(1041, 81, 1043, 83));
            holes.add_bounds(Bounds::new(256, 20, 768, 48));
            let mut sparse = DamageTiles::new(size);
            sparse.add_bounds(Bounds::new(8, 8, 9, 9));
            for (active, expected) in [
                (full, expected),
                (holes, if keyed { vec![7, 11] } else { vec![7, 11, 17] }),
                (sparse, if keyed { vec![7, 23] } else { vec![7, 17, 23] }),
                (DamageTiles::new(size), vec![]),
            ] {
                assert_eq!(query.collect(&bins, &canvas, &plan, &active), expected);
            }
            query.generation = u32::MAX;
            query.marks.fill(1);
            assert_eq!(
                query.collect(&bins, &canvas, &plan, &DamageTiles::full(size)),
                if keyed {
                    vec![3, 7, 11, 23]
                } else {
                    vec![3, 7, 11, 17, 23]
                }
            );
        }
    }
}

#[test]
fn active_batches_preserve_exact_sets_for_flat_and_paged_bins() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), Radius::ZERO, Color::BLACK);
    canvas.push_rect(Rect::new(16.0, 0.0, 32.0, 16.0), Radius::ZERO, Color::WHITE);
    canvas.push_rect(Rect::new(32.0, 0.0, 48.0, 16.0), Radius::ZERO, Color::BLACK);
    // More than one page in the first tile; the final draw has a distinct batch.
    for _ in 0..=COARSE_CHUNK_SIZE {
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::BLACK);
    }
    let mut plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut ids = vec![7; canvas.draw_records.len()];
    ids[1] = 3;
    ids[2] = u32::MAX;
    *ids.last_mut().unwrap() = 11;
    plan.draw_batch_ids = std::rc::Rc::new(ids);
    let size = (crate::TILE_SIZE * 3, crate::TILE_SIZE);
    for flat in [false, true] {
        let mut bins = build_tile_draw_bins(&canvas);
        let mut query = ActiveBatches::default();
        if flat {
            bins.reset_transient(
                &canvas.draw_records,
                &plan.draw_order,
                None,
                (3, 1),
                &mut Vec::new(),
                false,
            );
        }
        assert_eq!(bins.spatial_index_valid, !flat);
        let mut overlap = DamageTiles::new(size);
        for bounds in [
            Bounds::new(16, 0, 32, 16),
            Bounds::new(0, 0, 48, 16),
            Bounds::new(16, 0, 32, 16),
        ] {
            overlap.add_bounds(bounds);
        }
        assert_eq!(query.collect(&bins, &canvas, &plan, &overlap), [3, 7, 11]);
        let mut second = DamageTiles::new(size);
        second.add_bounds(Bounds::new(16, 0, 32, 16));
        assert_eq!(query.collect(&bins, &canvas, &plan, &second), [3, 7]);
        let mut inactive = DamageTiles::new(size);
        inactive.add_bounds(Bounds::new(32, 0, 48, 16));
        assert!(query.collect(&bins, &canvas, &plan, &inactive).is_empty());
        assert!(
            query
                .collect(&bins, &canvas, &plan, &DamageTiles::new(size))
                .is_empty()
        );
        query.generation = u32::MAX;
        query.marks.fill(1);
        assert_eq!(query.collect(&bins, &canvas, &plan, &overlap), [3, 7, 11]);
    }
}
