use peniko::Color;
use tileink::Canvas;

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 140;
pub const CLEAR: Color = Color::TRANSPARENT;

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="140" viewBox="0 0 240 140">
    <defs>
        <radialGradient id="ellipse" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="1"
            gradientTransform="translate(80 70) scale(64 30)">
            <stop offset="0" stop-color="#ffffff"/>
            <stop offset="0.55" stop-color="#22c55e"/>
            <stop offset="1" stop-color="#0f172a"/>
        </radialGradient>
        <linearGradient id="stroke" gradientUnits="userSpaceOnUse" x1="20" y1="0" x2="220" y2="0">
            <stop offset="0" stop-color="#ef4444"/>
            <stop offset="0.5" stop-color="#facc15"/>
            <stop offset="1" stop-color="#3b82f6"/>
        </linearGradient>
    </defs>
    <rect width="240" height="140" fill="#111827"/>
    <rect x="24" y="24" width="112" height="92" rx="12" fill="url(#ellipse)"/>
    <path d="M24 118 C72 82 120 154 216 56" fill="none" stroke="url(#stroke)"
        stroke-width="12" stroke-linecap="round"/>
</svg>"##;

pub fn scene() -> Result<Canvas, Box<dyn std::error::Error>> {
    let tree = usvg::Tree::from_str(SVG, &usvg::Options::default())?;
    let mut scene = Canvas::new(WIDTH, HEIGHT, 1.0);
    scene.push_svg(&tree)?;
    Ok(scene)
}
