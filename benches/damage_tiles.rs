use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tileink::{Bounds, DamageTilesBenchmark, TILE_SIZE};

const SIZE: (u32, u32) = (3840, 2160);

struct Case {
    name: &'static str,
    bounds: Vec<Bounds>,
}

impl Case {
    fn new(name: &'static str, bounds: Vec<Bounds>) -> Self {
        Self { name, bounds }
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case::new("one-tile", vec![Bounds::new(17, 17, 18, 18)]),
        Case::new(
            "sparse-disjoint-64",
            (0..64)
                .map(|index| {
                    let x = ((index * 53) % 230) * TILE_SIZE;
                    let y = ((index * 29) % 125) * TILE_SIZE;
                    Bounds::new(x as i32, y as i32, x as i32 + 8, y as i32 + 8)
                })
                .collect(),
        ),
        Case::new(
            "tiny-overlap-4096",
            (0..4096)
                .map(|index| {
                    let offset = index % 8;
                    Bounds::new(offset, offset, offset + 1, offset + 1)
                })
                .collect(),
        ),
        Case::new(
            "row-5-single",
            vec![Bounds::new(0, 0, 5 * TILE_SIZE as i32, TILE_SIZE as i32)],
        ),
        Case::new(
            "row-5-overlap-256",
            vec![Bounds::new(0, 0, 5 * TILE_SIZE as i32, TILE_SIZE as i32); 256],
        ),
        Case::new(
            "row-8-single",
            vec![Bounds::new(0, 0, 8 * TILE_SIZE as i32, TILE_SIZE as i32)],
        ),
        Case::new(
            "row-8-overlap-256",
            vec![Bounds::new(0, 0, 8 * TILE_SIZE as i32, TILE_SIZE as i32); 256],
        ),
        Case::new(
            "row-12-overlap-256",
            vec![Bounds::new(0, 0, 12 * TILE_SIZE as i32, TILE_SIZE as i32,); 256],
        ),
        Case::new(
            "medium-disjoint-96",
            (0..96)
                .map(|index| {
                    let x = (index % 12) * 20 * TILE_SIZE;
                    let y = (index / 12) * 16 * TILE_SIZE;
                    Bounds::new(
                        x as i32,
                        y as i32,
                        (x + 12 * TILE_SIZE) as i32,
                        (y + 8 * TILE_SIZE) as i32,
                    )
                })
                .collect(),
        ),
        Case::new(
            "medium-overlap-256",
            (0..256)
                .map(|index| {
                    let offset = index % 16;
                    Bounds::new(512 + offset, 512 + offset, 1536 + offset, 1536 + offset)
                })
                .collect(),
        ),
        Case::new(
            "wide-strips-128",
            (0..128)
                .map(|index| {
                    let y = (index % 64) * TILE_SIZE;
                    Bounds::new(0, y as i32, SIZE.0 as i32, (y + TILE_SIZE) as i32)
                })
                .collect(),
        ),
        Case::new(
            "large-single-70pct",
            vec![Bounds::new(0, 0, (SIZE.0 * 7 / 10) as i32, SIZE.1 as i32)],
        ),
        Case::new(
            "large-overlap-32",
            (0..32)
                .map(|index| {
                    let inset = index;
                    Bounds::new(inset, inset, SIZE.0 as i32 - inset, SIZE.1 as i32 - inset)
                })
                .collect(),
        ),
    ]
}

fn damage_tiles(c: &mut Criterion) {
    for case in cases() {
        let mut build = c.benchmark_group(format!("damage_tiles/build/{}", case.name));
        build.throughput(Throughput::Elements(case.bounds.len() as u64));
        build.bench_function(BenchmarkId::from_parameter("production"), |b| {
            b.iter(|| {
                let mut damage = DamageTilesBenchmark::new(SIZE);
                for &bounds in black_box(&case.bounds) {
                    damage.add_bounds(bounds);
                }
                black_box(damage.len())
            });
        });
        build.finish();

        let mut list = c.benchmark_group(format!("damage_tiles/build-list/{}", case.name));
        list.throughput(Throughput::Elements(case.bounds.len() as u64));
        list.bench_function(BenchmarkId::from_parameter("production"), |b| {
            b.iter(|| {
                let mut damage = DamageTilesBenchmark::new(SIZE);
                for &bounds in black_box(&case.bounds) {
                    damage.add_bounds(bounds);
                }
                black_box(damage.list().len())
            });
        });
        list.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(400))
        .measurement_time(Duration::from_secs(1));
    targets = damage_tiles
}
criterion_main!(benches);
