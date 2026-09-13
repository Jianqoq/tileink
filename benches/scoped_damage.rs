use criterion::{BenchmarkId, Criterion};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::{hint::black_box, path::Path, rc::Rc, time::Duration};
use tileink::{
    Canvas, Filter, Radius, Region, RetainedLayerDescriptor, RetainedMaterializerBenchmark,
    RetainedNodeId, RetainedParent, RetainedScene,
};

const ROOT: RetainedNodeId = RetainedNodeId::new(1, 0);
const OUTER: RetainedNodeId = RetainedNodeId::new(2, 0);
const SOURCE: RetainedNodeId = RetainedNodeId::new(3, 0);
const LEAF: RetainedNodeId = RetainedNodeId::new(4, 0);
fn region(rect: Rect) -> Region {
    Region::rect(rect, Radius::ZERO)
}
fn source_layer(amount: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Invert(amount),
        sample_region: region(Rect::new(0.0, 0.0, 8.0, 8.0)),
    }
}
fn leaf(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    Rc::new(canvas)
}
fn scene(count: usize, scoped: bool) -> RetainedScene {
    let mut scene = RetainedScene::new(1024, 1024, 1.0, ROOT).unwrap();
    let mut tx = scene.transaction();
    let parent = if scoped {
        tx.insert_layer(
            RetainedParent::content(ROOT),
            None,
            OUTER,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Offset { dx: 16.0, dy: 0.0 },
                sample_region: region(Rect::new(0.0, 0.0, 1024.0, 1024.0)),
            },
        );
        OUTER
    } else {
        ROOT
    };
    tx.insert_layer(
        RetainedParent::content(parent),
        None,
        SOURCE,
        source_layer(0.0),
    );
    tx.insert_scene(
        RetainedParent::content(SOURCE),
        None,
        LEAF,
        leaf(Color::from_rgb8(0, 0, 255)),
        Affine::IDENTITY,
    );
    for index in 0..count {
        let x = (index % 64) as f64 * 16.0;
        let y = (index / 64) as f64 * 16.0;
        tx.insert_layer(
            RetainedParent::content(parent),
            None,
            RetainedNodeId::new(index as u64 + 10, 0),
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: region(Rect::new(x, y, x + 8.0, y + 8.0)),
            },
        );
    }
    tx.commit().unwrap();
    scene
}
fn main() {
    let output = std::env::var("TILEINK_SCOPE_BENCH_OUTPUT")
        .expect("an explicit isolated Criterion output directory is required");
    let mut c = Criterion::default()
        .sample_size(60)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(8))
        .output_directory(Path::new(&output))
        .configure_from_args();
    let blue = leaf(Color::from_rgb8(0, 0, 255));
    let red = leaf(Color::from_rgb8(255, 0, 0));
    for (name, scoped, replace_leaf, counts) in [
        ("root-layer-parameter", false, false, &[0, 1, 100, 1000][..]),
        ("scoped-layer-parameter", true, false, &[1, 100, 1000][..]),
        ("scoped-leaf-revision", true, true, &[1, 100, 1000][..]),
    ] {
        let mut group = c.benchmark_group(name);
        for &count in counts {
            let mut scene = scene(count, scoped);
            let mut materializer = RetainedMaterializerBenchmark::new(&scene);
            let mut toggle = false;
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter(|| {
                    toggle = !toggle;
                    let mut tx = scene.transaction();
                    if replace_leaf {
                        tx.replace_scene(LEAF, if toggle { red.clone() } else { blue.clone() });
                    } else {
                        tx.update_layer(SOURCE, source_layer(if toggle { 1.0 } else { 0.0 }));
                    }
                    tx.commit().unwrap();
                    black_box(materializer.update_incremental(black_box(&scene)))
                })
            });
        }
        group.finish();
    }
    c.final_summary();
}
