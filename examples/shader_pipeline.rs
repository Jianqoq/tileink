//! Multi-pass native blur with half-resolution HDR intermediates and a tint image.
#[path = "common/shader_blur.rs"]
mod blur;
use peniko::{Color, kurbo::Rect};
use tileink::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer, Radius, Region,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(960, 640, 1.0);
    for y in (0..640).step_by(24) {
        for x in (0..960).step_by(24) {
            canvas.push_rect(
                Rect::new(x as f64, y as f64, (x + 24) as f64, (y + 24) as f64),
                Radius::ZERO,
                Color::from_rgb8(
                    (x / 4 % 256) as u8,
                    (80 + y / 4 % 176) as u8,
                    if (x / 24 + y / 24) % 2 == 0 { 180 } else { 70 },
                ),
            );
        }
    }
    for (x, tint) in [(48.0, [255; 4]), (512.0, [210, 235, 255, 255])] {
        canvas.push_backdrop_layer(
            blur::pipeline(2, 12.0, tint)?.into(),
            Region::rect(Rect::new(x, 80.0, x + 400.0, 560.0), Radius::all(48.0)),
        );
        canvas.pop_layer();
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
        .unwrap_or_else(|| "target/shader_pipeline.png".into());
    renderer.render_to_image(&canvas)?.readback()?.save(&path)?;
    println!("{path}");
    Ok(())
}
