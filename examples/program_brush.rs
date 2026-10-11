//! Two instances of one procedural paint; the right panel adds a point filter.
use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer, Radius, Region,
    ShaderInput, ShaderProgram, ShaderSources,
};
fn paint() -> Result<ShaderProgram, Box<dyn std::error::Error>> {
    Ok(ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("shaders/paint.metal").into()),
            hlsl: Some(include_str!("shaders/paint.hlsl").into()),
        },
        "paint",
        ShaderInput::None,
        8,
    )?)
}
fn scanlines() -> Result<ShaderProgram, Box<dyn std::error::Error>> {
    Ok(ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("shaders/scanlines.metal").into()),
            hlsl: Some(include_str!("shaders/scanlines.hlsl").into()),
        },
        "scanlines",
        ShaderInput::Current,
        0,
    )?)
}
fn scene() -> Result<Canvas, Box<dyn std::error::Error>> {
    let paint = paint()?;
    let scanlines = scanlines()?;
    let brush = paint.brush([0.021f32, 0.018, 0.0, 0.0, 0.8, 0.8, 0.8, 0.8].map(f32::to_bits))?;
    let mut scene = Canvas::new(960, 640, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 960.0, 640.0),
        Radius::ZERO,
        Color::from_rgb8(15, 18, 30),
    );
    for filtered in [false, true] {
        let mut panel = Canvas::new(432, 576, 1.0);
        let bounds = Rect::new(0.0, 0.0, 432.0, 576.0);
        if filtered {
            panel.push_filter_layer(
                scanlines.filter([])?.into(),
                Region::rect(bounds, Radius::all(30.0)),
            );
        }
        panel.push_rect(bounds, Radius::all(30.0), brush.clone());
        if filtered {
            panel.pop_layer();
        }
        scene.append(
            &panel,
            Point::new(if filtered { 496.0 } else { 32.0 }, 32.0),
        );
    }
    Ok(scene)
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
    let image = renderer.render_to_image(&scene()?)?.readback()?;
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/program_brush.png".into());
    image.save(&path)?;
    println!("{path}");
    Ok(())
}
