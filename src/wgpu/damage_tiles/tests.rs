use std::collections::{HashMap, HashSet};

use super::*;

fn bit_is_set(damage: &DamageTiles, tile: u32) -> bool {
    damage.bits[tile as usize / 64] & (1 << (tile % 64)) != 0
}

fn reference_count(damage: &DamageTiles, bounds: Bounds) -> u32 {
    if bounds.is_empty() {
        return 0;
    }
    let mut count = 0;
    for y in 0..damage.tiles_height {
        for x in 0..damage.tiles_width {
            let tile_bounds = Bounds::new(
                (x * TILE_SIZE) as i32,
                (y * TILE_SIZE) as i32,
                ((x + 1) * TILE_SIZE) as i32,
                ((y + 1) * TILE_SIZE) as i32,
            );
            if !bounds.intersect(tile_bounds).is_empty()
                && bit_is_set(damage, y * damage.tiles_width + x)
            {
                count += 1;
            }
        }
    }
    count
}

/// Straightforward per-tile implementation retained only as a semantic oracle.
fn reference_coalesced_rects(damage: &DamageTiles, physical_size: (u32, u32)) -> Vec<Bounds> {
    let mut rects = Vec::<Bounds>::new();
    let mut previous_row = HashMap::<(u32, u32), usize>::new();
    for y in 0..damage.tiles_height {
        let mut current_row = HashMap::new();
        let mut x = 0;
        while x < damage.tiles_width {
            while x < damage.tiles_width && !bit_is_set(damage, y * damage.tiles_width + x) {
                x += 1;
            }
            let start = x;
            while x < damage.tiles_width && bit_is_set(damage, y * damage.tiles_width + x) {
                x += 1;
            }
            if start == x {
                continue;
            }
            let key = (start, x);
            let y1 = ((y + 1) * TILE_SIZE).min(physical_size.1) as i32;
            let index = if let Some(&index) = previous_row.get(&key) {
                rects[index].y1 = y1;
                index
            } else {
                let index = rects.len();
                rects.push(Bounds::new(
                    (start * TILE_SIZE) as i32,
                    (y * TILE_SIZE) as i32,
                    (x * TILE_SIZE).min(physical_size.0) as i32,
                    y1,
                ));
                index
            };
            current_row.insert(key, index);
        }
        previous_row = current_row;
    }
    rects
}

fn next_random(seed: &mut u64) -> u32 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (*seed >> 32) as u32
}

#[test]
fn coalesced_rects_merge_adjacent_dirty_tile_rows() {
    let mut damage = DamageTiles::new((50, 20));
    damage.add_bounds(Bounds::new(15, 0, 34, 17));
    assert_eq!(
        damage.coalesced_rects((50, 20)),
        vec![Bounds::new(0, 0, 48, 20)]
    );
    assert_eq!(damage.count_in_bounds(Bounds::new(16, 0, 32, 16)), 1);
}

#[test]
fn full_sets_exact_non_word_aligned_tile_range() {
    let size = (64 * TILE_SIZE + 1, 2 * TILE_SIZE + 3);
    let damage = DamageTiles::full(size);

    assert_eq!(damage.dimensions(), (65, 3));
    assert_eq!(damage.list(), (0..195).collect::<Vec<_>>());
    assert_eq!(damage.bits, vec![u64::MAX, u64::MAX, u64::MAX, 0b111]);
    assert_eq!(
        damage.bounds_union(size),
        Some(Bounds::canvas(size.0, size.1))
    );
    assert_eq!(
        damage.coalesced_rects(size),
        vec![Bounds::canvas(size.0, size.1)]
    );

    let empty = DamageTiles::full((0, 100));
    assert!(empty.is_empty());
    assert!(empty.bits.is_empty());
    assert_eq!(empty.bounds_union((0, 100)), None);
}

#[test]
fn large_damage_crossing_words_keeps_exact_row_major_tiles() {
    let mut damage = DamageTiles::new((80 * TILE_SIZE, 2 * TILE_SIZE));
    damage.add_bounds(Bounds::new(
        4 * TILE_SIZE as i32,
        0,
        76 * TILE_SIZE as i32,
        2 * TILE_SIZE as i32,
    ));

    let expected = (0..2)
        .flat_map(|y| (4..76).map(move |x| y * 80 + x))
        .collect::<Vec<_>>();
    assert_eq!(damage.list(), expected);
    assert_eq!(damage.len(), 144);
}

#[test]
fn overlapping_large_damage_appends_only_new_tiles() {
    let mut damage = DamageTiles::new((80 * TILE_SIZE, 4 * TILE_SIZE));
    damage.add_bounds(Bounds::new(0, 0, 64 * TILE_SIZE as i32, TILE_SIZE as i32));
    damage.add_bounds(Bounds::new(
        16 * TILE_SIZE as i32,
        0,
        80 * TILE_SIZE as i32,
        TILE_SIZE as i32,
    ));

    assert_eq!(damage.list(), (0..80).collect::<Vec<_>>());
    assert_eq!(damage.len(), 80);
}

#[test]
fn narrow_and_large_damage_share_one_deduplicated_worklist() {
    let mut damage = DamageTiles::new((80 * TILE_SIZE, 2 * TILE_SIZE));
    damage.add_bounds(Bounds::new(
        70 * TILE_SIZE as i32,
        TILE_SIZE as i32,
        71 * TILE_SIZE as i32,
        2 * TILE_SIZE as i32,
    ));
    damage.add_bounds(Bounds::new(
        0,
        TILE_SIZE as i32,
        80 * TILE_SIZE as i32,
        2 * TILE_SIZE as i32,
    ));

    let expected = std::iter::once(150)
        .chain((80..160).filter(|tile| *tile != 150))
        .collect::<Vec<_>>();
    assert_eq!(damage.list(), expected);
    assert_eq!(damage.len(), 80);
}

#[test]
fn optimized_damage_matches_per_tile_reference_for_clipped_and_unaligned_bounds() {
    let size = (83 * TILE_SIZE + 7, 70 * TILE_SIZE + 3);
    let tiles_width = size.0.div_ceil(TILE_SIZE);
    let tiles_height = size.1.div_ceil(TILE_SIZE);
    let canvas = Bounds::canvas(tiles_width * TILE_SIZE, tiles_height * TILE_SIZE);
    let bounds = [
        Bounds::new(10, 10, 10, 40),
        Bounds::new(
            -300,
            -80,
            20 * TILE_SIZE as i32 + 3,
            40 * TILE_SIZE as i32 + 5,
        ),
        Bounds::new(
            57 * TILE_SIZE as i32 + 9,
            11,
            82 * TILE_SIZE as i32 + 2,
            19 * TILE_SIZE as i32 + 7,
        ),
        Bounds::new(
            5 * TILE_SIZE as i32,
            21 * TILE_SIZE as i32,
            6 * TILE_SIZE as i32,
            22 * TILE_SIZE as i32,
        ),
        Bounds::new(-1000, -1000, -1, -1),
        Bounds::new(-100, -100, size.0 as i32 + 100, size.1 as i32 + 100),
    ];
    let mut damage = DamageTiles::new(size);
    let mut expected = Vec::new();
    let mut seen = HashSet::new();

    for bounds in bounds {
        damage.add_bounds(bounds);
        let bounds = bounds.intersect(canvas);
        if !bounds.is_empty() {
            let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
            let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
            let x1 = (bounds.x1.max(0) as u32)
                .div_ceil(TILE_SIZE)
                .min(tiles_width);
            let y1 = (bounds.y1.max(0) as u32)
                .div_ceil(TILE_SIZE)
                .min(tiles_height);
            for y in y0..y1 {
                for x in x0..x1 {
                    let tile = y * tiles_width + x;
                    if seen.insert(tile) {
                        expected.push(tile);
                    }
                }
            }
        }
        assert_eq!(damage.list(), expected);
        assert_eq!(damage.len() as usize, seen.len());
    }
}

#[test]
fn word_queries_and_coalescing_match_per_tile_reference() {
    let size = (131 * TILE_SIZE + 7, 69 * TILE_SIZE + 3);
    let mut damage = DamageTiles::new(size);
    let mut seed = 0x4d59_5df4_d0f3_3173;

    for _ in 0..400 {
        let x0 = (next_random(&mut seed) % (size.0 + 512)) as i32 - 256;
        let y0 = (next_random(&mut seed) % (size.1 + 512)) as i32 - 256;
        let width = (next_random(&mut seed) % (40 * TILE_SIZE)) as i32;
        let height = (next_random(&mut seed) % (24 * TILE_SIZE)) as i32;
        damage.add_bounds(Bounds::new(x0, y0, x0 + width, y0 + height));
    }

    for _ in 0..500 {
        let x0 = (next_random(&mut seed) % (size.0 + 1024)) as i32 - 512;
        let y0 = (next_random(&mut seed) % (size.1 + 1024)) as i32 - 512;
        let width = (next_random(&mut seed) % (size.0 + 256)) as i32;
        let height = (next_random(&mut seed) % (size.1 + 256)) as i32;
        let bounds = Bounds::new(x0, y0, x0 + width, y0 + height);
        let expected = reference_count(&damage, bounds);
        assert_eq!(damage.count_in_bounds(bounds), expected, "{bounds:?}");
        assert_eq!(
            damage.intersects_bounds(bounds),
            expected != 0,
            "{bounds:?}"
        );
    }

    let expected_rects = reference_coalesced_rects(&damage, size);
    assert_eq!(damage.coalesced_rects(size), expected_rects);
    assert_eq!(
        damage.bounds_union(size),
        expected_rects.into_iter().reduce(Bounds::union)
    );
}

#[test]
fn bounds_union_tracks_disjoint_damage_without_scanning_the_gap() {
    let size = (1600, 1200);
    let mut damage = DamageTiles::new(size);
    let bounds = [
        Bounds::new(32, 16, 1500, 400),
        Bounds::new(640, 608, 672, 640),
        Bounds::new(100, 1100, 1900, 1300),
    ];

    for bounds in bounds {
        damage.add_bounds(bounds);
        assert_eq!(
            damage.bounds_union(size),
            damage
                .coalesced_rects(size)
                .into_iter()
                .reduce(Bounds::union)
        );
    }
    assert_eq!(
        damage.bounds_union(size),
        Some(Bounds::new(32, 16, 1600, 1200))
    );
}

#[test]
fn coalesced_rects_keep_different_row_runs_separate() {
    let mut damage = DamageTiles::new((64, 48));
    damage.add_bounds(Bounds::new(0, 0, 32, 32));
    damage.add_bounds(Bounds::new(32, 16, 48, 48));

    assert_eq!(
        damage.coalesced_rects((64, 48)),
        vec![
            Bounds::new(0, 0, 32, 16),
            Bounds::new(0, 16, 48, 32),
            Bounds::new(32, 32, 48, 48),
        ]
    );
}

#[test]
fn outset_includes_filter_source_tiles_beyond_visible_output() {
    let mut output = DamageTiles::new((328, 200));
    output.add_bounds(Bounds::new(112, 112, 256, 176));

    let source = output.outset((328, 200), 24);

    assert!(source.intersects_bounds(Bounds::new(112, 176, 256, 200)));
    assert_eq!(source.coalesced_rects((328, 200)).last().unwrap().y1, 200);
}
