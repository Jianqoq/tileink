use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{
    Canvas, Filter, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer,
    ProgressiveBlur, ProgressiveBlurQuality, Radius, Region,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let quality = match std::env::args().nth(2).as_deref() {
        None | Some("balanced") => ProgressiveBlurQuality::Balanced,
        Some("high") => ProgressiveBlurQuality::High,
        _ => return Err("quality must be balanced or high".into()),
    };
    let context = NativeContext::new(backend, &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, 960, 600)?;
    let mut canvas = Canvas::new(960, 600, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 960.0, 600.0),
        Radius::ZERO,
        Color::from_rgb8(20, 24, 34),
    );
    for y in 0..12 {
        for x in 0..20 {
            let color = [
                Color::from_rgb8(239, 106, 87),
                Color::from_rgb8(86, 185, 201),
                Color::from_rgb8(244, 201, 95),
            ][(x + y) % 3];
            canvas.push_rect(
                Rect::new(
                    x as f64 * 48.0 + 8.0,
                    y as f64 * 50.0 + 8.0,
                    x as f64 * 48.0 + 40.0,
                    y as f64 * 50.0 + 40.0,
                ),
                Radius::all(5.0),
                color,
            );
        }
    }
    canvas.push_backdrop_layer(
        Filter::ProgressiveBlur(
            ProgressiveBlur::new(Point::new(0.0, 140.0), Point::new(0.0, 460.0), 24.0)
                .with_quality(quality),
        ),
        Region::rect(Rect::new(0.0, 0.0, 960.0, 600.0), Radius::ZERO),
    );
    canvas.pop_layer();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/progressive-blur.png".into());
    renderer.render_to_image(&canvas)?.readback()?.save(&path)?;
    println!("Saved {path}");
    Ok(())
}
