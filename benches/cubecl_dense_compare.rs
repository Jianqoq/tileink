mod common;

use common::{HEIGHT, WIDTH, build_tileink_scene, prepared_cubecl_renderer, sync_cubecl};
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
    measurement::WallTime,
};
use peniko::Color;
use tileink::{CpuRenderer, CubeWgpuRenderer, Scene};

fn bench_prepared_cubecl_stage(
    group: &mut BenchmarkGroup<'_, WallTime>,
    scene: &Scene,
    path_count: usize,
    name: &'static str,
    warm: impl FnOnce(&mut CubeWgpuRenderer),
    mut run: impl FnMut(&mut CubeWgpuRenderer),
) {
    let mut renderer = prepared_cubecl_renderer(scene);
    warm(&mut renderer);
    sync_cubecl(&renderer);
    group.bench_with_input(BenchmarkId::new(name, path_count), &path_count, |b, _| {
        b.iter(|| {
            run(&mut renderer);
            sync_cubecl(&renderer);
        });
    });
}

fn benchmark_dense_case(c: &mut Criterion, path_count: usize) {
    let scene = build_tileink_scene(path_count, true);
    let mut group = c.benchmark_group("cubecl_dense_compare");
    group.throughput(Throughput::Elements((WIDTH * HEIGHT) as u64));

    {
        let mut renderer = CpuRenderer::new(WIDTH, HEIGHT, Color::WHITE);
        renderer.render(&scene);
        group.bench_with_input(
            BenchmarkId::new("cpu_full", path_count),
            &path_count,
            |b, _| {
                b.iter(|| {
                    renderer.render(black_box(&scene));
                    black_box(renderer.image().pixels[(WIDTH * HEIGHT / 2) as usize]);
                });
            },
        );
    }

    {
        let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::WHITE);
        renderer.prepare_scene(&scene);
        sync_cubecl(&renderer);
        group.bench_with_input(
            BenchmarkId::new("cubecl_prepare_scene", path_count),
            &path_count,
            |b, _| {
                b.iter(|| {
                    renderer.prepare_scene(black_box(&scene));
                    sync_cubecl(&renderer);
                });
            },
        );
    }

    bench_prepared_cubecl_stage(
        &mut group,
        &scene,
        path_count,
        "cubecl_scan_only",
        |renderer| renderer.scan(),
        |renderer| renderer.scan(),
    );

    bench_prepared_cubecl_stage(
        &mut group,
        &scene,
        path_count,
        "cubecl_scan_cumsum",
        |renderer| {
            renderer.scan();
            renderer.cumsum();
        },
        |renderer| {
            renderer.scan();
            renderer.cumsum();
        },
    );

    bench_prepared_cubecl_stage(
        &mut group,
        &scene,
        path_count,
        "cubecl_scan_cumsum_coarse",
        |renderer| {
            renderer.scan();
            renderer.cumsum();
            renderer.coarse();
        },
        |renderer| {
            renderer.scan();
            renderer.cumsum();
            renderer.coarse();
        },
    );

    bench_prepared_cubecl_stage(
        &mut group,
        &scene,
        path_count,
        "cubecl_fine_only",
        |renderer| {
            renderer.scan();
            renderer.cumsum();
            renderer.coarse();
            renderer.fine();
        },
        |renderer| renderer.fine(),
    );

    bench_prepared_cubecl_stage(
        &mut group,
        &scene,
        path_count,
        "cubecl_full_prepared",
        |renderer| {
            renderer.scan();
            renderer.cumsum();
            renderer.coarse();
            renderer.fine();
        },
        |renderer| {
            renderer.scan();
            renderer.cumsum();
            renderer.coarse();
            renderer.fine();
        },
    );

    {
        let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::WHITE);
        renderer.render(&scene);
        sync_cubecl(&renderer);
        group.bench_with_input(
            BenchmarkId::new("cubecl_full_with_prepare", path_count),
            &path_count,
            |b, _| {
                b.iter(|| {
                    renderer.render(black_box(&scene));
                    sync_cubecl(&renderer);
                });
            },
        );
    }

    group.finish();
}

fn cubecl_dense_compare(c: &mut Criterion) {
    for path_count in [64, 128, 256] {
        benchmark_dense_case(c, path_count);
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = cubecl_dense_compare
}
criterion_main!(benches);
