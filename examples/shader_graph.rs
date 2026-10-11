//! SourceGraphic + SourceAlpha + intermediate GPU results with a custom composite.
#[path = "common/shader_graph.rs"]
mod graph;
use peniko::{Color, kurbo::Rect};
use tileink::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(960, 640, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 960.0, 640.0),
        Radius::ZERO,
        Color::from_rgb8(225, 232, 241),
    );
    canvas.push_filter_layer(
        graph::shadow_graph(960, 640, true)?,
        Region::rect(Rect::new(0.0, 0.0, 960.0, 640.0), Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(120.0, 100.0, 580.0, 430.0),
        Radius::all(48.0),
        Color::from_rgb8(245, 133, 91),
    );
    canvas.push_rect(
        Rect::new(420.0, 230.0, 820.0, 520.0),
        Radius::all(72.0),
        Color::from_rgba8(80, 125, 238, 200),
    );
    canvas.pop_layer();
    let backend = if cfg!(feature = "metal") {
        NativeBackend::Metal
    } else if cfg!(feature = "vulkan") {
        NativeBackend::Vulkan
    } else {
        NativeBackend::Dx12
    };
    let context = NativeContext::new(backend, &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, 960, 640)?;
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/shader_graph.png".into());
    renderer.render_to_image(&canvas)?.readback()?.save(&path)?;
    println!("{path}");
    Ok(())
}
