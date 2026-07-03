#[path = "common/candlestick_scene.rs"]
mod candlestick_scene;
#[path = "common/mod.rs"]
mod common;
#[path = "common/emoji_scene.rs"]
mod emoji_scene;
#[path = "common/gradient_scene.rs"]
mod gradient_scene;
#[path = "common/layer_filter_scenes.rs"]
mod layer_filter_scenes;
#[path = "common/path_current_close_dash_scene.rs"]
mod path_current_close_dash_scene;
#[path = "common/path_text_scene.rs"]
mod path_text_scene;
#[path = "common/sdf_clip_scene.rs"]
mod sdf_clip_scene;
#[path = "common/sdf_dash_line_scene.rs"]
mod sdf_dash_line_scene;
#[path = "common/sdf_line_scene.rs"]
mod sdf_line_scene;
#[path = "common/sdf_nested_clip_blur_scene.rs"]
mod sdf_nested_clip_blur_scene;
#[path = "common/sdf_rect_shadow_scene.rs"]
mod sdf_rect_shadow_scene;
#[path = "common/sdf_shape_shadows_scene.rs"]
mod sdf_shape_shadows_scene;
#[path = "common/svg_radial_gradient_scene.rs"]
mod svg_radial_gradient_scene;
#[path = "common/text_scene.rs"]
mod text_scene;

use std::error::Error;

use peniko::{
    Color, Compose, Gradient, Mix,
    color::palette::css,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{Brush, Canvas, FillRule, Filter, Radius, StrokeWidths, TextContext, WgpuRenderer};

fn main() -> Result<(), Box<dyn Error>> {
    let mut batch = WgpuBatch::new();

    batch.render(
        "backdrop_blur",
        &layer_filter_scenes::backdrop_blur_scene(),
        layer_filter_scenes::CLEAR,
    )?;
    batch.render("blend", &blend_scene(), Color::WHITE)?;
    batch.render("blur", &blur_scene(), Color::WHITE)?;
    batch.render("brightness", &brightness_scene(), Color::WHITE)?;

    let (scene, _, _) = candlestick_scene::candlestick_scene();
    batch.render("candlestick", &scene, Color::WHITE)?;

    batch.render("clip", &clip_scene(), Color::WHITE)?;
    batch.render(
        "clip_filter",
        &layer_filter_scenes::clip_filter_scene(),
        layer_filter_scenes::CLEAR,
    )?;
    batch.render("contrast", &contrast_scene(), Color::WHITE)?;
    batch.render("drop_shadow", &drop_shadow_scene(), Color::WHITE)?;

    let mut text_context = TextContext::new();
    let scene = emoji_scene::scene(&mut text_context);
    batch.render_with_text("emoji", &scene, &mut text_context, emoji_scene::CLEAR)?;

    batch.render("even_odd", &even_odd_scene(), Color::WHITE)?;
    batch.render(
        "filter_clip_opacity",
        &layer_filter_scenes::filter_clip_opacity_scene(),
        layer_filter_scenes::CLEAR,
    )?;
    batch.render("filter_opacity", &filter_opacity_scene(), Color::WHITE)?;
    batch.render("gradients", &gradient_scene::scene(), gradient_scene::CLEAR)?;
    batch.render("grayscale", &grayscale_scene(), Color::WHITE)?;
    batch.render("hue_rotate", &hue_rotate_scene(), Color::WHITE)?;
    batch.render("invert", &invert_scene(), Color::WHITE)?;
    batch.render(
        "liquid_glass",
        &layer_filter_scenes::liquid_glass_scene(),
        layer_filter_scenes::CLEAR,
    )?;
    batch.render("opacity", &opacity_scene(), Color::WHITE)?;

    let (scene, _, _) = path_current_close_dash_scene::path_current_close_dash_scene();
    batch.render("path_current_close_dash", &scene, Color::WHITE)?;

    let scene = path_text_scene::scene(&mut text_context);
    batch.render_with_text(
        "path_text",
        &scene,
        &mut text_context,
        path_text_scene::CLEAR,
    )?;

    batch.render(
        "rect_stroke_widths",
        &rect_stroke_widths_scene(),
        Color::WHITE,
    )?;
    batch.render("rounded_rect", &rounded_rect_scene(), Color::WHITE)?;
    batch.render("saturate", &saturate_scene(), Color::WHITE)?;

    let (scene, _, _) = sdf_clip_scene::sdf_clip_scene();
    batch.render("sdf_clip", &scene, Color::WHITE)?;

    let (scene, _, _) = sdf_dash_line_scene::sdf_dash_line_scene();
    batch.render("sdf_dash_line", &scene, Color::WHITE)?;

    let (scene, _, _) = sdf_line_scene::sdf_line_scene();
    batch.render("sdf_line", &scene, Color::WHITE)?;

    let (scene, _, _) = sdf_nested_clip_blur_scene::sdf_nested_clip_blur_scene();
    batch.render("sdf_nested_clip_blur", &scene, Color::WHITE)?;

    let (scene, _, _) = sdf_rect_shadow_scene::sdf_rect_shadow_scene();
    batch.render("sdf_rect_shadow", &scene, Color::WHITE)?;

    let (scene, _, _) = sdf_shape_shadows_scene::sdf_shape_shadows_scene();
    batch.render("sdf_shape_shadows", &scene, Color::WHITE)?;

    batch.render("sepia", &sepia_scene(), Color::WHITE)?;
    batch.render("simple", &simple_scene(), Color::WHITE)?;
    batch.render("stroke_circle", &stroke_circle_scene(), Color::WHITE)?;

    let scene = svg_radial_gradient_scene::scene()?;
    batch.render(
        "svg_radial_gradient",
        &scene,
        svg_radial_gradient_scene::CLEAR,
    )?;

    let (scene, _, _) = common::load_svg_scene(common::example_asset("tiger.svg"), 900)?;
    batch.render("svg_tiger", &scene, Color::TRANSPARENT)?;

    for case in &text_scene::CASES {
        let scene = text_scene::scene(&mut text_context, case);
        batch.render_with_text(case.name, &scene, &mut text_context, case.background)?;
    }

    Ok(())
}

struct WgpuBatch {
    renderer: WgpuRenderer,
}

impl WgpuBatch {
    fn new() -> Self {
        Self {
            renderer: WgpuRenderer::new_default_device(
                common::EXAMPLE_WIDTH,
                common::EXAMPLE_HEIGHT,
                Color::TRANSPARENT,
            ),
        }
    }

    fn render(&mut self, name: &str, scene: &Canvas, clear: Color) -> Result<(), Box<dyn Error>> {
        self.renderer.set_clear_color(clear);
        self.renderer.render(scene);
        self.save(name)
    }

    fn render_with_text(
        &mut self,
        name: &str,
        scene: &Canvas,
        text_context: &mut TextContext,
        clear: Color,
    ) -> Result<(), Box<dyn Error>> {
        self.renderer.set_clear_color(clear);
        self.renderer.render_with_text(scene, text_context);
        self.save(name)
    }

    fn save(&self, name: &str) -> Result<(), Box<dyn Error>> {
        let image = self.renderer.image();
        let out = common::wgpu_example_output(name);
        common::save_example_image(&image, &out)?;
        println!("Wrote {}", out.display());
        Ok(())
    }
}

fn simple_scene() -> Canvas {
    let width = 360;
    let height = 260;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(42.0, 38.0, 178.0, 128.0),
        Radius::ZERO,
        Color::from_rgba8(37, 143, 93, 230),
    );
    scene.push_path(
        Circle::new((242.0, 86.0), 56.0).to_path(0.1),
        Color::from_rgba8(45, 111, 211, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Rect::new(72.0, 156.0, 178.0, 218.0),
        Stroke::new(12.0),
        Color::from_rgb8(230, 89, 80),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Circle::new((260.0, 184.0), 40.0),
        Stroke::new(10.0),
        Color::from_rgb8(222, 178, 106),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
}

fn stroke_circle_scene() -> Canvas {
    let width = 220;
    let height = 180;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(250, 250, 248),
    );
    scene.push_stroke(
        Circle::new((110.0, 90.0), 54.0),
        Stroke::new(16.0),
        Color::from_rgb8(222, 96, 72),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
}

fn rect_stroke_widths_scene() -> Canvas {
    let width = 480;
    let height = 320;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(24.0, 24.0, 456.0, 296.0),
        Radius::ZERO,
        Color::from_rgb8(235, 239, 244),
    );
    scene.push_rect_stroke_widths(
        Rect::new(58.0, 58.0, 210.0, 150.0),
        Radius::ZERO,
        StrokeWidths {
            top: 4.0,
            right: 22.0,
            bottom: 12.0,
            left: 34.0,
        },
        Color::from_rgb8(220, 64, 72),
    );
    scene.push_rect_stroke_widths(
        Rect::new(282.0, 54.0, 424.0, 154.0),
        Radius::all(22.0),
        StrokeWidths {
            top: 18.0,
            right: 6.0,
            bottom: 28.0,
            left: 12.0,
        },
        Color::from_rgb8(49, 112, 214),
    );
    scene.push_rect_stroke_widths(
        Rect::new(92.0, 204.0, 388.0, 258.0),
        Radius::all(10.0),
        StrokeWidths {
            top: 8.0,
            right: 32.0,
            bottom: 8.0,
            left: 16.0,
        },
        Color::from_rgb8(28, 153, 104),
    );
    scene
}

fn blur_scene() -> Canvas {
    let mut scene = Canvas::new(1920, 1080);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 1920.0, 1080.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 28.0,
            std_dev_y: 28.0,
            sampling: Default::default(),
        },
        common::canvas_region(1920, 1080),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((880.0, 520.0), 250.0),
        Color::from_rgba8(37, 99, 235, 230),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(800.0, 360.0, 1240.0, 700.0),
        Radius::ZERO,
        Color::from_rgba8(220, 38, 38, 210),
    );
    scene.pop_layer();
    common::fill_circle(&mut scene, Circle::new((880.0, 520.0), 128.0), Color::WHITE);
    scene
}

fn blend_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(150.0, 80.0, 360.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    scene.push_blend_layer(
        common::rect_path(Rect::new(0.0, 0.0, 640.0, 360.0), Radius::ZERO),
        Affine::IDENTITY,
        0.1,
        Mix::Multiply,
        Compose::SrcOver,
    );
    common::fill_rect(
        &mut scene,
        Rect::new(280.0, 120.0, 500.0, 300.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    scene
}

fn brightness_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(70.0, 80.0, 280.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 64, 175),
    );
    scene.push_filter_layer(Filter::Brightness(1.65), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 80.0, 570.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 64, 175),
    );
    scene.pop_layer();
    scene
}

fn contrast_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(80.0, 80.0, 250.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(96, 165, 250),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(170.0, 80.0, 300.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 41, 59),
    );
    scene.push_filter_layer(Filter::Contrast(1.8), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 80.0, 530.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(96, 165, 250),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(450.0, 80.0, 580.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 41, 59),
    );
    scene.pop_layer();
    scene
}

fn grayscale_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Grayscale(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 80.0, 330.0, 260.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((360.0, 180.0), 88.0),
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 110.0, 530.0, 250.0),
        Radius::ZERO,
        Color::from_rgb8(22, 163, 74),
    );
    scene.pop_layer();
    scene
}

fn hue_rotate_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::HueRotate(120.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(90.0, 80.0, 230.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(250.0, 80.0, 390.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(410.0, 80.0, 550.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(22, 163, 74),
    );
    scene.pop_layer();
    scene
}

fn invert_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Invert(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(110.0, 70.0, 530.0, 290.0),
        Radius::ZERO,
        Color::from_rgb8(15, 23, 42),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((320.0, 180.0), 82.0),
        Color::from_rgb8(245, 158, 11),
    );
    scene.pop_layer();
    scene
}

fn saturate_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Saturate(2.4), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(90.0, 90.0, 250.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(129, 140, 248),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(250.0, 90.0, 410.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(45, 212, 191),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(410.0, 90.0, 550.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(251, 146, 60),
    );
    scene.pop_layer();
    scene
}

fn sepia_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Sepia(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 76.0, 520.0, 284.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((320.0, 180.0), 76.0),
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    scene
}

fn drop_shadow_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    let shadow =
        Gradient::new_linear((180.0, 0.0), (440.0, 0.0)).with_stops([css::MAGENTA, css::BLUE]);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 26.0,
            offset_y: 24.0,
            std_dev: 12.0,
            brush: Brush::from_gradient(&shadow),
        },
        common::canvas_region(640, 360),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((300.0, 160.0), 88.0),
        Color::from_rgb8(37, 99, 235),
    );
    scene.pop_layer();
    scene
}

fn filter_opacity_scene() -> Canvas {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 90.0, 520.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(226, 232, 240),
    );
    scene.push_filter_layer(Filter::Opacity(0.45), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(180.0, 70.0, 460.0, 290.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    scene
}

fn opacity_scene() -> Canvas {
    let mut scene = Canvas::new(360, 260);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(52.0, 56.0, 210.0, 204.0),
        Radius::ZERO,
        Color::from_rgb8(226, 232, 240),
    );
    scene.push_opacity_layer(
        common::rect_path(Rect::new(0.0, 0.0, 360.0, 260.0), Radius::ZERO),
        Affine::IDENTITY,
        0.1,
        0.45,
    );
    common::fill_rect(
        &mut scene,
        Rect::new(92.0, 76.0, 264.0, 168.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((236.0, 146.0), 58.0),
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    common::fill_rect(
        &mut scene,
        Rect::new(232.0, 42.0, 308.0, 94.0),
        Radius::ZERO,
        Color::from_rgb8(22, 163, 74),
    );
    scene
}

fn clip_scene() -> Canvas {
    let mut scene = Canvas::new(360, 260);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::stroke_circle(
        &mut scene,
        Circle::new((180.0, 130.0), 74.0),
        Stroke::new(3.0),
        Color::from_rgb8(31, 41, 55),
    );
    scene.push_clip_layer(
        Circle::new((180.0, 130.0), 74.0).to_path(0.1),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let clip_path = Rect::new(118.0, 82.0, 300.0, 178.0).to_path(0.1);
    let clip_stroke_path = Rect::new(118.5, 82.5, 299.5, 177.5).to_path(0.1);
    let complex = complex_clip_path();
    scene.push_clip_layer(clip_path, Affine::IDENTITY, FillRule::NonZero, 0.1);
    scene.push_clip_layer(complex.clone(), Affine::IDENTITY, FillRule::NonZero, 0.1);
    common::fill_rect(
        &mut scene,
        Rect::new(64.0, 56.0, 330.0, 204.0),
        Radius::ZERO,
        Color::from_rgba8(14, 165, 233, 235),
    );
    scene.pop_layer();
    scene.pop_layer();
    scene.pop_layer();
    scene.push_stroke(
        clip_stroke_path,
        Stroke::new(3.0),
        Color::from_rgb8(100, 116, 139),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        complex,
        Stroke::new(2.0),
        Color::from_rgb8(220, 38, 38),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
}

fn complex_clip_path() -> BezPath {
    let mut path = BezPath::new();
    path.move_to((124.0, 128.0));
    path.curve_to((134.0, 96.0), (166.0, 86.0), (184.0, 102.0));
    path.curve_to((202.0, 74.0), (252.0, 82.0), (270.0, 114.0));
    path.curve_to((304.0, 124.0), (290.0, 164.0), (258.0, 164.0));
    path.curve_to((238.0, 190.0), (194.0, 182.0), (178.0, 158.0));
    path.curve_to((154.0, 178.0), (126.0, 160.0), (124.0, 128.0));
    path.close_path();
    path
}

fn even_odd_scene() -> Canvas {
    let mut scene = Canvas::new(520, 280);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 520.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_path(
        nested_rect_path(30.0),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    common::stroke_rect(
        &mut scene,
        Rect::new(60.5, 60.5, 220.5, 220.5),
        Radius::ZERO,
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene.push_path(
        nested_rect_path(270.0),
        Color::from_rgb8(220, 38, 38),
        Affine::IDENTITY,
        FillRule::EvenOdd,
        0.1,
    );
    common::stroke_rect(
        &mut scene,
        Rect::new(300.5, 60.5, 460.5, 220.5),
        Radius::ZERO,
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene
}

fn nested_rect_path(offset_x: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((offset_x + 30.0, 60.0));
    path.line_to((offset_x + 190.0, 60.0));
    path.line_to((offset_x + 190.0, 220.0));
    path.line_to((offset_x + 30.0, 220.0));
    path.close_path();
    path.move_to((offset_x + 78.0, 108.0));
    path.line_to((offset_x + 142.0, 108.0));
    path.line_to((offset_x + 142.0, 172.0));
    path.line_to((offset_x + 78.0, 172.0));
    path.close_path();
    path
}

fn rounded_rect_scene() -> Canvas {
    let mut scene = Canvas::new(480, 320);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 480.0, 320.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(48.0, 48.0, 280.0, 200.0),
        Radius {
            top_left: 36.0,
            top_right: 36.0,
            bottom_left: 8.0,
            bottom_right: 8.0,
        },
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(300.0, 56.0, 432.0, 188.0),
        Radius {
            top_left: 8.0,
            top_right: 40.0,
            bottom_left: 40.0,
            bottom_right: 8.0,
        },
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(56.0, 208.0, 424.0, 288.0),
        Radius {
            top_left: 12.0,
            top_right: 12.0,
            bottom_left: 28.0,
            bottom_right: 28.0,
        },
        Color::from_rgb8(22, 163, 74),
    );
    scene
}
