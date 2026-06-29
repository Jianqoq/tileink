mod common;

use common::{
    HEIGHT, VelloWgpuContext, WIDTH, build_tileink_scene, circle_at, color_at,
    prepared_cubecl_renderer, sync_cubecl, vello_renderer,
};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use cubecl::prelude::Runtime;
use tileink::{CubePreparedStage, CubeRenderer, CubeWgpuRenderer, Scene};

fn build_vello_scene(path_count: usize, dense: bool) -> vello::Scene {
    let mut scene = vello::Scene::new();
    for i in 0..path_count {
        scene.fill(
            vello::peniko::Fill::NonZero,
            vello::kurbo::Affine::IDENTITY,
            color_at(i),
            None,
            &circle_at(i, dense),
        );
    }
    scene
}

fn run_cubecl_prepared<R: Runtime>(renderer: &mut CubeRenderer<R>, scene: &Scene) {
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Scan);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Cumsum);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Coarse);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Fine);
}

fn benchmark_case(c: &mut Criterion, path_count: usize, dense: bool) {
    let tileink_scene = build_tileink_scene(path_count, dense);
    let mut cubecl_renderer = prepared_cubecl_renderer(&tileink_scene);
    run_cubecl_prepared(&mut cubecl_renderer, &tileink_scene);
    sync_cubecl(&cubecl_renderer);

    let vello_context = VelloWgpuContext::new(WIDTH, HEIGHT, "tileink_vello_compare");
    let vello_scene = build_vello_scene(path_count, dense);
    let mut vello_renderer = vello_renderer(&vello_context.device);
    let vello_params = vello::RenderParams {
        base_color: vello::peniko::Color::WHITE,
        width: WIDTH,
        height: HEIGHT,
        antialiasing_method: vello::AaConfig::Area,
    };
    vello_renderer
        .render_to_texture(
            &vello_context.device,
            &vello_context.queue,
            &vello_scene,
            &vello_context.target_view,
            &vello_params,
        )
        .expect("Vello warmup");
    vello_context.sync();

    let scenario = if dense { "dense" } else { "distributed" };
    let mut group = c.benchmark_group(format!("gpu_compare/{scenario}"));
    group.throughput(Throughput::Elements((WIDTH * HEIGHT) as u64));

    group.bench_with_input(
        BenchmarkId::new("cubecl_gpu_prepared", path_count),
        &path_count,
        |b, _| {
            b.iter(|| {
                run_cubecl_prepared(black_box(&mut cubecl_renderer), black_box(&tileink_scene));
                sync_cubecl(&cubecl_renderer);
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("cubecl_gpu_with_prepare", path_count),
        &path_count,
        |b, _| {
            let mut renderer =
                CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, vello::peniko::Color::WHITE);
            b.iter(|| {
                renderer.render(black_box(&tileink_scene));
                sync_cubecl(&renderer);
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("vello_gpu_area", path_count),
        &path_count,
        |b, _| {
            b.iter(|| {
                vello_renderer
                    .render_to_texture(
                        black_box(&vello_context.device),
                        black_box(&vello_context.queue),
                        black_box(&vello_scene),
                        black_box(&vello_context.target_view),
                        black_box(&vello_params),
                    )
                    .expect("Vello render");
                vello_context.sync();
            });
        },
    );

    group.finish();
}

fn gpu_compare(c: &mut Criterion) {
    benchmark_case(c, 64, false);
    benchmark_case(c, 256, false);
    benchmark_case(c, 64, true);
    benchmark_case(c, 128, true);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = gpu_compare
}
criterion_main!(benches);
