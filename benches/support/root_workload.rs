use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius};

fn root_batches(width: u32, height: u32, batches: u32, sparse: bool) -> Canvas {
    let mut canvas = Canvas::new(width, height, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        Radius::ZERO,
        Color::from_rgb8(12, 18, 28),
    );
    for index in 0..batches {
        let inset = f64::from(index) * f64::from(width.min(height)) / f64::from(batches * 4);
        let rect = Rect::new(
            inset,
            inset,
            f64::from(width) - inset,
            f64::from(height) - inset,
        );
        canvas.push_clip_sdf_rect_layer(rect, Radius::all(8.0));
        canvas.push_rect(
            if sparse {
                Rect::new(inset + 8.0, inset + 8.0, inset + 40.0, inset + 40.0)
            } else {
                rect
            },
            Radius::ZERO,
            if index % 2 == 0 {
                Color::from_rgba8(28, 108, 228, 18)
            } else {
                Color::from_rgba8(74, 198, 148, 18)
            },
        );
        canvas.pop_layer();
    }
    canvas
}

pub fn benchmark_scenes() -> Vec<(String, Canvas, u32)> {
    let mut scenes = Vec::new();
    for (width, height, batches) in [1, 2, 4, 8, 16, 32, 64]
        .map(|batches| (1600, 1000, batches))
        .into_iter()
        .chain([
            (800, 500, 32),
            (1601, 1001, 32),
            (2560, 1440, 32),
            (3840, 2160, 32),
        ])
    {
        for sparse in [false, true] {
            if sparse && batches == 1 {
                continue;
            }
            let kind = if sparse { "sparse" } else { "dense" };
            scenes.push((
                format!("{kind}-{width}x{height}-b{batches}"),
                root_batches(width, height, batches - 1, sparse),
                batches,
            ));
        }
    }
    let tiger = usvg::Tree::from_data(
        include_bytes!("../../examples/tiger.svg"),
        &usvg::Options::default(),
    )
    .unwrap();
    for (size, batches) in [400, 800, 1600, 2048, 2304, 3200]
        .map(|size| (size, 1))
        .into_iter()
        .chain(
            [800, 1600, 3200]
                .into_iter()
                .flat_map(|size| [2, 8].map(|batches| (size, batches))),
        )
    {
        let mut canvas = Canvas::new(size, size, 1.0);
        for layer in 0..batches {
            if batches > 1 {
                let inset = f64::from(layer) * 0.5;
                canvas.push_clip_sdf_rect_layer(
                    Rect::new(
                        inset,
                        inset,
                        f64::from(size) - inset,
                        f64::from(size) - inset,
                    ),
                    Radius::all(4.0),
                );
            }
            canvas
                .push_svg_with_options(
                    &tiger,
                    tileink::SvgOptions {
                        transform: peniko::kurbo::Affine::scale_non_uniform(
                            f64::from(size) / f64::from(tiger.size().width()),
                            f64::from(size) / f64::from(tiger.size().height()),
                        ),
                        ..tileink::SvgOptions::default()
                    },
                )
                .unwrap();
            if batches > 1 {
                canvas.pop_layer();
            }
        }
        scenes.push((format!("tiger-{size}-b{batches}"), canvas, batches));
    }
    scenes
}
