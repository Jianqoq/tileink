//! Native mesh brushes on a clipped panel, a path, and a transparent foreground.
use peniko::{
    Color,
    kurbo::{Affine, BezPath, Rect},
};
use tileink::{
    Canvas, FillRule, MeshGradient, NativeBackend, NativeContext, NativeContextOptions,
    NativeRenderer, Radius,
};

#[allow(
    clippy::expect_used,
    reason = "The example uses fixed valid rectangular mesh dimensions, positions and colors"
)]
fn scene() -> Canvas {
    let mut canvas = Canvas::new(960, 640, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 960.0, 640.0),
        Radius::ZERO,
        Color::from_rgb8(15, 18, 30),
    );
    let positions = [
        [32.0, 32.0],
        [480.0, 32.0],
        [928.0, 32.0],
        [32.0, 320.0],
        [580.0, 245.0],
        [928.0, 320.0],
        [32.0, 608.0],
        [480.0, 608.0],
        [928.0, 608.0],
    ];
    let colors = [
        Color::from_rgb8(30, 50, 160),
        Color::from_rgb8(220, 55, 145),
        Color::from_rgb8(255, 170, 90),
        Color::from_rgb8(20, 180, 205),
        Color::from_rgb8(130, 70, 240),
        Color::from_rgb8(255, 90, 105),
        Color::from_rgb8(15, 45, 100),
        Color::from_rgb8(60, 130, 220),
        Color::from_rgb8(230, 95, 150),
    ];
    canvas.push_rect(
        Rect::new(32.0, 32.0, 928.0, 608.0),
        Radius::all(32.0),
        MeshGradient::new(3, 3, positions, colors).expect(
            "The example uses fixed valid rectangular mesh dimensions, positions and colors",
        ),
    );
    let mut path = BezPath::new();
    path.move_to((110.0, 440.0));
    path.curve_to((230.0, 300.0), (320.0, 600.0), (460.0, 445.0));
    path.line_to((460.0, 545.0));
    path.line_to((110.0, 545.0));
    path.close_path();
    let foreground = MeshGradient::new(
        2,
        2,
        [
            [110.0, 300.0],
            [460.0, 300.0],
            [110.0, 545.0],
            [460.0, 545.0],
        ],
        [
            Color::from_rgba8(255, 255, 255, 210),
            Color::from_rgba8(80, 220, 255, 180),
            Color::TRANSPARENT,
            Color::from_rgba8(255, 255, 255, 60),
        ],
    )
    .expect("The example uses fixed valid rectangular mesh dimensions, positions and colors");
    canvas.push_path(path, foreground, Affine::IDENTITY, FillRule::NonZero, 0.1);
    canvas
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = if cfg!(feature = "metal") {
        NativeBackend::Metal
    } else if cfg!(feature = "vulkan") {
        NativeBackend::Vulkan
    } else {
        NativeBackend::Dx12
    };
    let context = NativeContext::new(backend, &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, 960, 640)?;
    let image = renderer.render_to_image(&scene())?.readback()?;
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/mesh_gradient.png".into());
    image.save(&path)?;
    println!("{path}");
    Ok(())
}

#[test]
fn mesh_example_builds_a_native_scene() {
    assert_eq!(scene().physical_size(), (960, 640));
}
