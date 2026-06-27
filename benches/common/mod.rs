use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape},
};
use tileink::{CubeWgpuRenderer, FillRule, Scene};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

pub fn color_at(i: usize) -> Color {
    Color::from_rgba8(
        40 + (i * 17 % 150) as u8,
        70 + (i * 11 % 150) as u8,
        210,
        170,
    )
}

pub fn circle_at(i: usize, dense: bool) -> Circle {
    if dense {
        Circle::new(
            (
                WIDTH as f64 * 0.5 + (i % 7) as f64 - 3.0,
                HEIGHT as f64 * 0.5 + (i % 5) as f64 - 2.0,
            ),
            245.0 - (i % 11) as f64,
        )
    } else {
        let x = i % 20;
        let y = i / 20;
        Circle::new((32.0 + x as f64 * 61.0, 32.0 + y as f64 * 52.0), 22.0)
    }
}

pub fn build_tileink_scene(path_count: usize, dense: bool) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        Color::WHITE,
        FillRule::NonZero,
    );

    for i in 0..path_count {
        scene.push_circle(circle_at(i, dense), color_at(i), FillRule::NonZero);
    }

    scene
}

pub fn prepared_cubecl_renderer(scene: &Scene) -> CubeWgpuRenderer {
    let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::WHITE);
    // Prepared stage benchmarks intentionally exclude scene upload and buffer preallocation.
    renderer.prepare_scene(scene);
    sync_cubecl(&renderer);
    renderer
}

pub fn sync_cubecl(renderer: &CubeWgpuRenderer) {
    // CubeCL launches asynchronously; sync measures GPU completion without target readback.
    cubecl_common::future::block_on(renderer.client().sync()).expect("CubeCL sync failed");
}
