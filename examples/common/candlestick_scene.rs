use peniko::{Color, kurbo::Rect};
use tileink::{CandleStick, FillRule, Scene};

pub fn candlestick_scene() -> (Scene, u32, u32) {
    let width = 520;
    let height = 320;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(36.0, 32.0, 484.0, 284.0),
        Color::from_rgb8(255, 255, 255),
        FillRule::NonZero,
    );

    for y in [72.0, 112.0, 152.0, 192.0, 232.0] {
        scene.push_rect(
            Rect::new(36.0, y, 484.0, y + 1.0),
            Color::from_rgb8(228, 233, 240),
            FillRule::NonZero,
        );
    }

    let candles = [
        (64.5, 88.0, 210.0, 132.0, 188.0, false),
        (96.5, 96.0, 224.0, 178.0, 124.0, true),
        (128.5, 70.0, 184.0, 112.0, 162.0, false),
        (160.5, 82.0, 210.0, 172.0, 130.0, true),
        (192.5, 78.0, 238.0, 146.0, 216.0, false),
        (224.5, 104.0, 244.0, 218.0, 172.0, true),
        (256.5, 84.0, 202.0, 166.0, 118.0, true),
        (288.5, 64.0, 194.0, 110.0, 154.0, false),
        (320.5, 74.0, 218.0, 160.0, 102.0, true),
        (352.5, 92.0, 236.0, 124.0, 204.0, false),
        (384.5, 86.0, 228.0, 198.0, 144.0, true),
        (416.5, 62.0, 178.0, 136.0, 92.0, true),
        (448.5, 78.0, 216.0, 104.0, 190.0, false),
    ];

    for (center_x, high_y, low_y, open_y, close_y, up) in candles {
        let color = if up {
            Color::from_rgb8(22, 163, 74)
        } else {
            Color::from_rgb8(220, 64, 72)
        };
        scene.push_candlestick(
            CandleStick::new(center_x, high_y, low_y, open_y, close_y, 11),
            color,
            FillRule::NonZero,
        );
    }

    (scene, width, height)
}
