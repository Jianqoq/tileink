use super::*;

#[test]
fn active_region_does_not_join_unrelated_dirty_tiles_across_a_clean_gap() {
    let (canvas, _) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(0, 0, 1, 1));
    damage.add_bounds(Bounds::new(31, 31, 32, 32));
    a.retained.set_active_tiles(Some(damage));
    assert_eq!(active_region(&a, Bounds::new(16, 0, 32, 16)), None);
}

#[test]
fn active_region_preserves_filter_bounds_and_leaves_tile_masking_to_the_adapter() {
    let (canvas, _) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    let mut damage = DamageTiles::new((32, 32));
    damage.add_bounds(Bounds::new(0, 0, 1, 1));
    a.retained.set_active_tiles(Some(damage));
    assert_eq!(
        active_region(&a, Bounds::canvas(32, 32)),
        Some(Bounds::canvas(32, 32))
    );
}

#[test]
fn active_region_distinguishes_complete_and_empty_redraws() {
    let (canvas, _) = fixture();
    let mut a = Adapter::new(&canvas, RetainedSurfaceKind::Backdrop);
    assert_eq!(
        active_region(&a, Bounds::canvas(32, 32)),
        Some(Bounds::canvas(32, 32))
    );
    a.retained
        .set_active_tiles(Some(DamageTiles::new((32, 32))));
    assert_eq!(active_region(&a, Bounds::canvas(32, 32)), None);
}
