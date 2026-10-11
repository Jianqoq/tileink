//! Query indices come from one complete scene domain, including hidden layer records.
use super::super::*;
use super::*;
use crate::{FillRule, Filter, Radius, Region, RetainedNodeId, RetainedParent, RetainedScene};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

#[test]
fn generated_damage_and_bins_share_the_complete_membership_domain() {
    for size in [(1, 1), (17, 17), (1043, 87)] {
        let rect = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let mut source = Canvas::new(size.0, size.1, 1.0);
        source.push_clip_layer(rect.to_path(0.1), Affine::IDENTITY, FillRule::NonZero, 0.1);
        source.push_rect(rect, Radius::ZERO, Color::BLACK);
        source.push_filter_layer(Filter::Opacity(0.5), Region::rect(rect, Radius::ZERO));
        source.push_rect(rect, Radius::ZERO, Color::WHITE);
        source.pop_layer();
        source.push_rect(rect, Radius::ZERO, Color::BLACK);
        source.pop_layer();
        let root = RetainedNodeId::for_owner(970_000);
        let mut retained = RetainedScene::new(size.0, size.1, 1.0, root).unwrap();
        retained
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(970_001),
                Rc::new(source.clone()),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        let materializer = crate::retained_scene::PersistentSceneMaterializer::new(&retained);
        for canvas in [Rc::new(source), materializer.canvas()] {
            let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
            let ids = canvas.raster_batch_ids(&plan);
            assert_eq!(ids.len(), canvas.draw_records.len());
            assert!(
                ids.contains(&u32::MAX),
                "hidden layer records have no raster batch"
            );
            let mut expected = ids
                .iter()
                .copied()
                .filter(|&id| id != u32::MAX)
                .collect::<Vec<_>>();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(expected.len(), 3);
            for persistent in [false, true] {
                let mut bins = TileDrawBins::default();
                let dimensions = (canvas.width_in_tiles(), canvas.height_in_tiles());
                let keys = canvas
                    .retained_painter()
                    .map(|painter| painter.keys.as_slice());
                if persistent {
                    bins.reset(&canvas.draw_records, &plan.draw_order, keys, dimensions);
                } else {
                    bins.reset_transient(
                        &canvas.draw_records,
                        &plan.draw_order,
                        keys,
                        dimensions,
                        &mut Vec::new(),
                        false,
                    );
                }
                let mut active = DamageTiles::new(size);
                active.add_bounds(Bounds::new(
                    -100,
                    -100,
                    size.0 as i32 + 100,
                    size.1 as i32 + 100,
                ));
                assert_eq!(active.dimensions(), dimensions);
                assert_eq!(active.len() as usize, bins.records.len());
                for &tile in active.list() {
                    bins.for_each_tile_draw(tile as usize, |draw| {
                        assert_ne!(ids[draw as usize], u32::MAX);
                    });
                }
                let mut query = ActiveBatches::default();
                assert_eq!(query.collect(&bins, &canvas, &plan, &active), expected);
                assert!(
                    query
                        .collect(&bins, &canvas, &plan, &DamageTiles::new(size))
                        .is_empty()
                );
            }
        }
    }
}
