#[path = "backdrop_blur.rs"]
mod backdrop_blur;
#[path = "blend.rs"]
mod blend;
#[path = "blur.rs"]
mod blur;
#[path = "brightness.rs"]
mod brightness;
#[path = "candlestick.rs"]
mod candlestick;
#[path = "clip.rs"]
mod clip;
#[path = "clip_filter.rs"]
mod clip_filter;
#[path = "contrast.rs"]
mod contrast;
#[path = "downsample_blur.rs"]
mod downsample_blur;
#[path = "drop_shadow.rs"]
mod drop_shadow;
#[path = "emoji.rs"]
mod emoji;
#[path = "even_odd.rs"]
mod even_odd;
#[path = "filter_clip_opacity.rs"]
mod filter_clip_opacity;
#[path = "filter_opacity.rs"]
mod filter_opacity;
#[path = "gradients.rs"]
mod gradients;
#[path = "grayscale.rs"]
mod grayscale;
#[path = "hue_rotate.rs"]
mod hue_rotate;
#[path = "invert.rs"]
mod invert;
#[path = "liquid_glass.rs"]
mod liquid_glass;
#[path = "liquid_glass_fast_path.rs"]
mod liquid_glass_fast_path;
#[path = "opacity.rs"]
mod opacity;
#[path = "path_current_close_dash.rs"]
mod path_current_close_dash;
#[path = "path_text.rs"]
mod path_text;
#[path = "rect_stroke_widths.rs"]
mod rect_stroke_widths;
#[path = "rounded_rect.rs"]
mod rounded_rect;
#[path = "saturate.rs"]
mod saturate;
#[path = "sdf_clip.rs"]
mod sdf_clip;
#[path = "sdf_dash_line.rs"]
mod sdf_dash_line;
#[path = "sdf_line.rs"]
mod sdf_line;
#[path = "sdf_nested_clip_blur.rs"]
mod sdf_nested_clip_blur;
#[path = "sdf_rect_shadow.rs"]
mod sdf_rect_shadow;
#[path = "sdf_shape_shadows.rs"]
mod sdf_shape_shadows;
#[path = "sepia.rs"]
mod sepia;
#[path = "simple.rs"]
mod simple;
#[path = "stroke_circle.rs"]
mod stroke_circle;
#[path = "svg_radial_gradient.rs"]
mod svg_radial_gradient;
#[path = "svg_tiger.rs"]
mod svg_tiger;
#[path = "text.rs"]
mod text;

type ExampleFn = fn() -> Result<(), Box<dyn std::error::Error>>;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Expected example images: {} ({} external SVG inputs)",
        OUTPUTS.len(),
        SVG_INPUTS.len()
    );
    let examples: &[(&str, ExampleFn)] = &[
        ("backdrop_blur", backdrop_blur::run),
        ("blend", blend::run),
        ("blur", blur::run),
        ("brightness", brightness::run),
        ("candlestick", candlestick::run),
        ("clip", clip::run),
        ("clip_filter", clip_filter::run),
        ("contrast", contrast::run),
        ("downsample_blur", downsample_blur::run),
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
        println!("Running WGPU example: {name}");
        run()?;
    }
    Ok(())
}

/// All current PNG outputs; the retired sdf_triangle image is not a case.
pub const OUTPUTS: &[&str] = &[
    "backdrop_blur",
    "blend",
    "blur",
    "brightness",
    "candlestick",
    "clip",
    "clip_filter",
    "contrast",
    "downsample_blur",
    "drop_shadow",
    "emoji",
    "even_odd",
    "filter_clip_opacity",
    "filter_opacity",
    "gradients",
    "grayscale",
    "hue_rotate",
    "invert",
    "liquid_glass",
    "liquid_glass_fast_path_default",
    "liquid_glass_fast_path_mixed",
    "liquid_glass_fast_path_simple",
    "opacity",
    "path_current_close_dash",
    "path_text",
    "rect_stroke_widths",
    "rounded_rect",
    "saturate",
    "sdf_clip",
    "sdf_dash_line",
    "sdf_line",
    "sdf_nested_clip_blur",
    "sdf_rect_shadow",
    "sdf_shape_shadows",
    "sepia",
    "simple",
    "stroke_circle",
    "svg_radial_gradient",
    "svg_tiger",
    "text_black_on_white_12px",
    "text_black_on_white_16px",
    "text_black_on_white_24px",
    "text_white_on_black_12px",
    "text_white_on_black_16px",
    "text_white_on_black_24px",
];

/// External SVG assets opened by this suite, relative to examples/.
pub const SVG_INPUTS: &[&str] = &["tiger.svg"];
