mod common;

use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use common::{VelloWgpuContext, sync_cubecl, vello_renderer};
use criterion::{
    BenchmarkGroup, Criterion, Throughput, black_box, criterion_group, criterion_main,
    measurement::WallTime,
};
use cubecl::prelude::Runtime;
use peniko::kurbo::Affine as TileAffine;
#[cfg(feature = "cuda")]
use tileink::CubeCudaRenderer;
use tileink::{CubePreparedStage, CubeRenderer, CubeWgpuRenderer, Scene, SvgOptions};
use usvg::{Node, Paint, PaintOrder, tiny_skia_path::PathSegment};
use vello::{
    kurbo::{Affine, BezPath, Cap, Join, Stroke},
    peniko::{Color, Fill},
};

const TARGET_WIDTH: u32 = 900;

type BenchResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct TigerBenchError(String);

impl TigerBenchError {
    fn unsupported(feature: impl Into<String>) -> Self {
        Self(format!(
            "unsupported tiger SVG feature for Vello benchmark: {}",
            feature.into()
        ))
    }
}

impl fmt::Display for TigerBenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for TigerBenchError {}

fn tiger_svg_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tiger.svg")
}

fn load_tiger_tree(path: &Path) -> BenchResult<usvg::Tree> {
    let data = fs::read(path)?;
    let options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    Ok(usvg::Tree::from_data(&data, &options)?)
}

fn target_size(tree: &usvg::Tree) -> BenchResult<(u32, u32)> {
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(TARGET_WIDTH)
        .ok_or_else(|| TigerBenchError::unsupported("non-positive SVG size"))?;
    Ok((size.width(), size.height()))
}

fn tileink_scale(tree: &usvg::Tree, width: u32, height: u32) -> TileAffine {
    TileAffine::scale_non_uniform(
        width as f64 / tree.size().width() as f64,
        height as f64 / tree.size().height() as f64,
    )
}

fn vello_scale(tree: &usvg::Tree, width: u32, height: u32) -> Affine {
    Affine::scale_non_uniform(
        width as f64 / tree.size().width() as f64,
        height as f64 / tree.size().height() as f64,
    )
}

fn build_tileink_scene(tree: &usvg::Tree, width: u32, height: u32) -> BenchResult<Scene> {
    let mut scene = Scene::new(width, height);
    scene.push_svg_with_options(
        tree,
        SvgOptions {
            transform: tileink_scale(tree, width, height),
            ..SvgOptions::default()
        },
    )?;
    Ok(scene)
}

fn build_vello_scene(tree: &usvg::Tree, width: u32, height: u32) -> BenchResult<vello::Scene> {
    let mut scene = vello::Scene::new();
    // Keep the benchmark honest: Vello lowering supports the solid path subset used by tiger.svg
    // and rejects any unsupported SVG construct instead of silently rendering a different scene.
    push_vello_group(&mut scene, tree.root(), vello_scale(tree, width, height))?;
    Ok(scene)
}

fn push_vello_group(
    scene: &mut vello::Scene,
    group: &usvg::Group,
    base_transform: Affine,
) -> BenchResult<()> {
    if group.opacity().get() < 1.0 {
        return Err(TigerBenchError::unsupported("group opacity").into());
    }
    if group.blend_mode() != usvg::BlendMode::Normal {
        return Err(TigerBenchError::unsupported("group blend mode").into());
    }
    if group.clip_path().is_some() || group.mask().is_some() || !group.filters().is_empty() {
        return Err(TigerBenchError::unsupported("group clip/mask/filter").into());
    }

    for child in group.children() {
        match child {
            Node::Group(group) => push_vello_group(scene, group, base_transform)?,
            Node::Path(path) => push_vello_path(scene, path, base_transform)?,
            Node::Text(_) => return Err(TigerBenchError::unsupported("text node").into()),
            Node::Image(_) => return Err(TigerBenchError::unsupported("image node").into()),
        }
    }
    Ok(())
}

fn push_vello_path(
    scene: &mut vello::Scene,
    path: &usvg::Path,
    base_transform: Affine,
) -> BenchResult<()> {
    if !path.is_visible() {
        return Ok(());
    }

    let data = tiny_path_to_bez(path.data());
    let transform = base_transform * transform_to_affine(path.abs_transform());
    match path.paint_order() {
        PaintOrder::FillAndStroke => {
            push_vello_fill(scene, path, &data, transform)?;
            push_vello_stroke(scene, path, &data, transform)?;
        }
        PaintOrder::StrokeAndFill => {
            push_vello_stroke(scene, path, &data, transform)?;
            push_vello_fill(scene, path, &data, transform)?;
        }
    }
    Ok(())
}

fn push_vello_fill(
    scene: &mut vello::Scene,
    path: &usvg::Path,
    data: &BezPath,
    transform: Affine,
) -> BenchResult<()> {
    let Some(fill) = path.fill() else {
        return Ok(());
    };
    if fill.opacity().get() <= 0.0 {
        return Ok(());
    }

    scene.fill(
        fill_rule(fill.rule()),
        transform,
        solid_paint_color(fill.paint(), fill.opacity().get(), "fill paint")?,
        None,
        data,
    );
    Ok(())
}

fn push_vello_stroke(
    scene: &mut vello::Scene,
    path: &usvg::Path,
    data: &BezPath,
    transform: Affine,
) -> BenchResult<()> {
    let Some(stroke) = path.stroke() else {
        return Ok(());
    };
    if stroke.opacity().get() <= 0.0 {
        return Ok(());
    }
    if stroke.linejoin() == usvg::LineJoin::MiterClip {
        return Err(TigerBenchError::unsupported("miter-clip stroke join").into());
    }

    scene.stroke(
        &stroke_to_kurbo(stroke),
        transform,
        solid_paint_color(stroke.paint(), stroke.opacity().get(), "stroke paint")?,
        None,
        data,
    );
    Ok(())
}

fn solid_paint_color(paint: &Paint, opacity: f32, feature: &str) -> BenchResult<Color> {
    match paint {
        Paint::Color(color) => {
            Ok(Color::from_rgb8(color.red, color.green, color.blue).multiply_alpha(opacity))
        }
        Paint::LinearGradient(_) => {
            Err(TigerBenchError::unsupported(format!("linear gradient {feature}")).into())
        }
        Paint::RadialGradient(_) => {
            Err(TigerBenchError::unsupported(format!("radial gradient {feature}")).into())
        }
        Paint::Pattern(_) => Err(TigerBenchError::unsupported(format!("pattern {feature}")).into()),
    }
}

fn stroke_to_kurbo(stroke: &usvg::Stroke) -> Stroke {
    let mut out = Stroke::new(stroke.width().get() as f64)
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter => Join::Miter,
            usvg::LineJoin::Round => Join::Round,
            usvg::LineJoin::Bevel => Join::Bevel,
            usvg::LineJoin::MiterClip => unreachable!("miter-clip is rejected before lowering"),
        })
        .with_miter_limit(stroke.miterlimit().get() as f64)
        .with_caps(match stroke.linecap() {
            usvg::LineCap::Butt => Cap::Butt,
            usvg::LineCap::Round => Cap::Round,
            usvg::LineCap::Square => Cap::Square,
        });

    if let Some(dasharray) = stroke.dasharray() {
        out = out.with_dashes(
            stroke.dashoffset() as f64,
            dasharray.iter().map(|dash| *dash as f64),
        );
    }
    out
}

fn tiny_path_to_bez(path: &usvg::tiny_skia_path::Path) -> BezPath {
    let mut out = BezPath::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => out.move_to((p.x as f64, p.y as f64)),
            PathSegment::LineTo(p) => out.line_to((p.x as f64, p.y as f64)),
            PathSegment::QuadTo(p0, p1) => {
                out.quad_to((p0.x as f64, p0.y as f64), (p1.x as f64, p1.y as f64));
            }
            PathSegment::CubicTo(p0, p1, p2) => out.curve_to(
                (p0.x as f64, p0.y as f64),
                (p1.x as f64, p1.y as f64),
                (p2.x as f64, p2.y as f64),
            ),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

fn transform_to_affine(transform: usvg::Transform) -> Affine {
    Affine::new([
        transform.sx as f64,
        transform.ky as f64,
        transform.kx as f64,
        transform.sy as f64,
        transform.tx as f64,
        transform.ty as f64,
    ])
}

fn fill_rule(rule: usvg::FillRule) -> Fill {
    match rule {
        usvg::FillRule::NonZero => Fill::NonZero,
        usvg::FillRule::EvenOdd => Fill::EvenOdd,
    }
}

fn run_cubecl_prepared<R: Runtime>(renderer: &mut CubeRenderer<R>, scene: &Scene) {
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Scan);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Cumsum);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Coarse);
    renderer.run_prepared_stage_for_bench(scene, CubePreparedStage::Fine);
}

fn bench_wgpu_stage(
    group: &mut BenchmarkGroup<'_, WallTime>,
    scene: &Scene,
    width: u32,
    height: u32,
    name: &'static str,
    warm: impl FnOnce(&mut CubeWgpuRenderer),
    mut run: impl FnMut(&mut CubeWgpuRenderer),
) {
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);
    renderer.prepare_scene_for_bench(scene);
    warm(&mut renderer);
    sync_cubecl(&renderer);
    group.bench_function(name, |b| {
        b.iter(|| {
            run(black_box(&mut renderer));
            sync_cubecl(&renderer);
        });
    });
}

fn tiger_gpu_compare(c: &mut Criterion) {
    let tree = load_tiger_tree(&tiger_svg_path()).expect("load examples/tiger.svg");
    let (width, height) = target_size(&tree).expect("compute tiger target size");
    let tileink_scene = build_tileink_scene(&tree, width, height).expect("lower tiger to tileink");
    let vello_scene = build_vello_scene(&tree, width, height).expect("lower tiger to Vello");

    let mut cubecl_renderer =
        CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);
    cubecl_renderer.prepare_scene_for_bench(&tileink_scene);
    run_cubecl_prepared(&mut cubecl_renderer, &tileink_scene);
    sync_cubecl(&cubecl_renderer);

    #[cfg(feature = "cuda")]
    let mut cuda_renderer = {
        let mut renderer = CubeCudaRenderer::new_default_device(width, height, Color::TRANSPARENT);
        renderer.prepare_scene_for_bench(&tileink_scene);
        run_cubecl_prepared(&mut renderer, &tileink_scene);
        sync_cubecl(&renderer);
        renderer
    };

    let vello_context = VelloWgpuContext::new(width, height, "tiger_vello_compare");
    let mut vello_renderer = vello_renderer(&vello_context.device);
    let vello_params = vello::RenderParams {
        base_color: Color::TRANSPARENT,
        width,
        height,
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
        .expect("Vello tiger warmup");
    vello_context.sync();

    let mut group = c.benchmark_group("tiger_gpu_compare");
    group.throughput(Throughput::Elements((width * height) as u64));

    group.bench_function("cubecl_gpu_prepared", |b| {
        b.iter(|| {
            run_cubecl_prepared(black_box(&mut cubecl_renderer), black_box(&tileink_scene));
            sync_cubecl(&cubecl_renderer);
        });
    });

    bench_wgpu_stage(
        &mut group,
        &tileink_scene,
        width,
        height,
        "cubecl_gpu_stage_scan",
        |renderer| renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Scan),
        |renderer| renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Scan),
    );

    bench_wgpu_stage(
        &mut group,
        &tileink_scene,
        width,
        height,
        "cubecl_gpu_stage_cumsum",
        |renderer| {
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Scan);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Cumsum);
        },
        |renderer| renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Cumsum),
    );

    bench_wgpu_stage(
        &mut group,
        &tileink_scene,
        width,
        height,
        "cubecl_gpu_stage_coarse",
        |renderer| {
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Scan);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Cumsum);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Coarse);
        },
        |renderer| renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Coarse),
    );

    bench_wgpu_stage(
        &mut group,
        &tileink_scene,
        width,
        height,
        "cubecl_gpu_stage_fine",
        |renderer| {
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Scan);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Cumsum);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Coarse);
            renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Fine);
        },
        |renderer| renderer.run_prepared_stage_for_bench(&tileink_scene, CubePreparedStage::Fine),
    );

    group.bench_function("cubecl_gpu_with_prepare", |b| {
        let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);
        b.iter(|| {
            renderer.render(black_box(&tileink_scene));
            sync_cubecl(&renderer);
        });
    });

    #[cfg(feature = "cuda")]
    group.bench_function("cubecl_cuda_prepared", |b| {
        b.iter(|| {
            run_cubecl_prepared(black_box(&mut cuda_renderer), black_box(&tileink_scene));
            sync_cubecl(&cuda_renderer);
        });
    });

    #[cfg(feature = "cuda")]
    group.bench_function("cubecl_cuda_with_prepare", |b| {
        let mut renderer = CubeCudaRenderer::new_default_device(width, height, Color::TRANSPARENT);
        b.iter(|| {
            renderer.render(black_box(&tileink_scene));
            sync_cubecl(&renderer);
        });
    });

    group.bench_function("vello_gpu_area", |b| {
        b.iter(|| {
            vello_renderer
                .render_to_texture(
                    black_box(&vello_context.device),
                    black_box(&vello_context.queue),
                    black_box(&vello_scene),
                    black_box(&vello_context.target_view),
                    black_box(&vello_params),
                )
                .expect("Vello tiger render");
            vello_context.sync();
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = tiger_gpu_compare
}
criterion_main!(benches);
