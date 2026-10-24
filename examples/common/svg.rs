//! Native SVG fixture loading with bundled fonts.
use peniko::kurbo::Affine;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tileink::{Canvas, SvgOptions};

pub fn load_svg_scene(
    input: impl AsRef<Path>,
    target_width: u32,
) -> Result<(Canvas, u32, u32), Box<dyn std::error::Error>> {
    let input = input.as_ref();
    let data = fs::read(input)?;
    let mut options = svg_options();
    options.resources_dir = input.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, &options)?;
    svg_tree_to_scene(&tree, target_width)
}

pub fn svg_options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    load_svg_fonts(&mut options);
    // SVGs without a usable font-family still need a face from the bundled font database.
    "Noto Serif".clone_into(&mut options.font_family);
    options
}

pub fn svg_tree_to_scene(
    tree: &usvg::Tree,
    target_width: u32,
) -> Result<(Canvas, u32, u32), Box<dyn std::error::Error>> {
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(target_width)
        .ok_or("SVG size must be positive")?;
    let width = size.width();
    let height = size.height();
    let scale_x = f64::from(width) / f64::from(tree.size().width());
    let scale_y = f64::from(height) / f64::from(tree.size().height());
    let mut scene = Canvas::new(width, height, 1.0);
    scene.push_svg_with_options(
        tree,
        SvgOptions {
            transform: Affine::scale_non_uniform(scale_x, scale_y),
            ..SvgOptions::default()
        },
    )?;
    Ok((scene, width, height))
}

fn load_svg_fonts(options: &mut usvg::Options<'_>) {
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("svg")
        .join("fonts");
    let fontdb = options.fontdb_mut();
    fontdb.load_fonts_dir(fonts_dir);
    // Resolve generic SVG families to bundled faces; platform defaults may not be installed.
    fontdb.set_serif_family("Noto Serif");
    fontdb.set_sans_serif_family("Noto Sans");
    fontdb.set_monospace_family("Noto Mono");
    fontdb.set_cursive_family("Yellowtail");
    fontdb.set_fantasy_family("Sedgwick Ave Display");
}
