#[path = "../common/mod.rs"]
mod common;
#[path = "../common/layer_filter_scenes.rs"]
mod layer_filter_scenes;

mod backdrop_blur;
mod blend;
mod blur;
mod brightness;
mod candlestick;
mod clip;
mod clip_filter;
mod contrast;
mod drop_shadow;
mod emoji;
mod even_odd;
mod filter_clip_opacity;
mod filter_opacity;
mod gradients;
mod grayscale;
mod hue_rotate;
mod invert;
mod liquid_glass;
mod liquid_glass_fast_path;
mod opacity;
mod path_current_close_dash;
mod path_text;
mod rect_stroke_widths;
mod rounded_rect;
mod saturate;
mod sdf_clip;
mod sdf_dash_line;
mod sdf_line;
mod sdf_nested_clip_blur;
mod sdf_rect_shadow;
mod sdf_shape_shadows;
mod sepia;
mod simple;
mod stroke_circle;
mod svg_radial_gradient;
mod svg_tiger;
mod text;

type ExampleFn = fn() -> Result<(), Box<dyn std::error::Error>>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let examples: &[(&str, ExampleFn)] = &[
        ("backdrop_blur", backdrop_blur::run),
        ("blend", blend::run),
        ("blur", blur::run),
        ("brightness", brightness::run),
        ("candlestick", candlestick::run),
        ("clip", clip::run),
        ("clip_filter", clip_filter::run),
        ("contrast", contrast::run),
        ("drop_shadow", drop_shadow::run),
        ("emoji", emoji::run),
        ("even_odd", even_odd::run),
        ("filter_clip_opacity", filter_clip_opacity::run),
        ("filter_opacity", filter_opacity::run),
        ("gradients", gradients::run),
        ("grayscale", grayscale::run),
        ("hue_rotate", hue_rotate::run),
        ("invert", invert::run),
        ("liquid_glass", liquid_glass::run),
        ("liquid_glass_fast_path", liquid_glass_fast_path::run),
        ("opacity", opacity::run),
        ("path_current_close_dash", path_current_close_dash::run),
        ("path_text", path_text::run),
        ("rect_stroke_widths", rect_stroke_widths::run),
        ("rounded_rect", rounded_rect::run),
        ("saturate", saturate::run),
        ("sdf_clip", sdf_clip::run),
        ("sdf_dash_line", sdf_dash_line::run),
        ("sdf_line", sdf_line::run),
        ("sdf_nested_clip_blur", sdf_nested_clip_blur::run),
        ("sdf_rect_shadow", sdf_rect_shadow::run),
        ("sdf_shape_shadows", sdf_shape_shadows::run),
        ("sepia", sepia::run),
        ("simple", simple::run),
        ("stroke_circle", stroke_circle::run),
        ("svg_radial_gradient", svg_radial_gradient::run),
        ("svg_tiger", svg_tiger::run),
        ("text", text::run),
    ];
    for (name, run) in examples {
        println!("Running CPU example: {name}");
        run()?;
    }
    Ok(())
}
