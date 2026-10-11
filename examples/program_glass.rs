//! Real backdrop refraction expressed entirely through the public shader API.
//! Both panels are custom filters; the right one adds RGB dispersion.
#[path = "common/program_glass.rs"]
mod glass;
use peniko::{Color, kurbo::Rect};
use tileink::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer, Radius, Region,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(960, 640, 1.0);
    for y in (0..640).step_by(24) {
        for x in (0..960).step_by(24) {
            let color = Color::from_rgb8(
                (x / 4 % 256) as u8,
                (80 + y / 4 % 176) as u8,
                if (x / 24 + y / 24) % 2 == 0 { 180 } else { 70 },
            );
            scene.push_rect(
                Rect::new(x as f64, y as f64, (x + 24) as f64, (y + 24) as f64),
                Radius::ZERO,
                color,
            );
        }
    }
    for (cx, dispersion) in [(248.0, false), (712.0, true)] {
        let program = glass::program()?;
        let uniforms = glass::uniforms([cx, 320.0], [200.0, 240.0], 48.0, dispersion);
        let filter = program.filter(uniforms)?;
        scene.push_backdrop_layer(
            filter.into(),
            Region::rect(
                Rect::new(cx as f64 - 200.0, 80.0, cx as f64 + 200.0, 560.0),
                Radius::all(48.0),
            ),
        );
        scene.pop_layer();
    }
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
        .unwrap_or_else(|| "target/program_glass.png".into());
    renderer.render_to_image(&scene)?.readback()?.save(&path)?;
    println!("{path}");
    Ok(())
}
