use peniko::Color;

use super::*;
use crate::{CpuRenderer, WgpuRenderer};

fn parse(svg: &str) -> usvg::Tree {
    usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap()
}

fn render(svg: &str, clear: Color) -> CpuRenderer {
    let tree = parse(svg);
    let size = tree.size();
    render_tree_with_options(
        &tree,
        clear,
        SvgOptions::default(),
        size.width().ceil() as u32,
        size.height().ceil() as u32,
    )
}

fn render_with_options(
    svg: &str,
    clear: Color,
    options: SvgOptions,
    width: u32,
    height: u32,
) -> CpuRenderer {
    let tree = parse(svg);
    render_tree_with_options(&tree, clear, options, width, height)
}

fn render_tree_with_options(
    tree: &usvg::Tree,
    clear: Color,
    options: SvgOptions,
    width: u32,
    height: u32,
) -> CpuRenderer {
    let mut canvas = Canvas::new(width, height);
    canvas.push_svg_with_options(tree, options).unwrap();
    let mut renderer = CpuRenderer::new(canvas.width, canvas.height, clear);
    renderer.render(&canvas);
    renderer
}

fn assert_rgba_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= tolerance),
        "actual {actual:?}, expected {expected:?}"
    );
}

fn run_wgpu_svg_tests() -> bool {
    std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() == Ok("1")
        || std::env::var("TILEINK_RUN_WGPU_SVG_TESTS").as_deref() == Ok("1")
}

fn assert_images_exact(
    expected: &crate::shared::image::Image,
    actual: &crate::shared::image::Image,
    context: &str,
) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    let mut mismatch_count = 0usize;
    let mut first_mismatch = None;
    for (ix, (&expected_px, &actual_px)) in expected.pixels.iter().zip(&actual.pixels).enumerate() {
        if expected_px != actual_px {
            mismatch_count += 1;
            if first_mismatch.is_none() {
                let x = ix as u32 % expected.width;
                let y = ix as u32 / expected.width;
                first_mismatch = Some((x, y, expected.rgba8_at(x, y), actual.rgba8_at(x, y)));
            }
        }
    }
    assert_eq!(
        mismatch_count, 0,
        "{context} has {mismatch_count} pixel differences; first mismatch: {first_mismatch:?}"
    );
}

const OPAQUE_RED_BLUE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
const TRANSLUCENT_ORANGE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP438DQAAAGAQIADTyPKQAAAABJRU5ErkJggg==";

fn encoded_test_image(format: ::image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    let image = ::image::RgbImage::from_raw(2, 1, vec![255, 0, 0, 0, 0, 255]).unwrap();
    ::image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn push_svg_renders_basic_fill_and_stroke() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <rect x="4" y="4" width="18" height="18" fill="#ff0000" stroke="#0000ff" stroke-width="4"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(12, 12), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(4, 12), [0, 0, 255, 255]);
}

#[test]
fn push_svg_native_wgpu_matches_cpu_exact_when_enabled() {
    if !run_wgpu_svg_tests() {
        return;
    }

    let tree = parse(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <rect x="2" y="2" width="12" height="18" fill="#0a141e"/>
                <rect x="14" y="6" width="10" height="10" fill="#dc4050"/>
                <path d="M4 26 L28 26 L28 30 L4 30 Z" fill="#2060a0"/>
            </svg>"##,
    );
    let mut canvas = Canvas::new(32, 32);
    canvas.push_svg(&tree).unwrap();

    let mut cpu = CpuRenderer::new(32, 32, Color::TRANSPARENT);
    cpu.render(&canvas);

    let mut wgpu = WgpuRenderer::new_default_device(32, 32, Color::TRANSPARENT);
    assert!(
        wgpu.render_native(&canvas),
        "expected SVG canvas to render through native wgpu path"
    );
    let wgpu_image = wgpu.image();

    assert_images_exact(cpu.image(), &wgpu_image, "svg native wgpu vs cpu");
}

#[test]
fn push_svg_native_wgpu_matches_cpu_for_path_text_fixture_when_enabled() {
    if !run_wgpu_svg_tests() {
        return;
    }

    let canvas = svg_fixture_scene("text/textPath/writing-mode=tb.svg", 300);
    assert_eq!(
        canvas.text_glyphs.len(),
        0,
        "SVG text fixtures render as paths"
    );

    let mut cpu = CpuRenderer::new(canvas.width, canvas.height, Color::TRANSPARENT);
    let mut wgpu =
        WgpuRenderer::new_default_device(canvas.width, canvas.height, Color::TRANSPARENT);
    for pass in 0..5 {
        cpu.render(&canvas);
        assert!(
            wgpu.render_native(&canvas),
            "expected SVG path text fixture to render through native wgpu path"
        );
        let wgpu_image = wgpu.image();
        assert_images_exact(
            cpu.image(),
            &wgpu_image,
            &format!("svg path text native wgpu vs cpu pass {pass}"),
        );
    }
}

fn svg_fixture_scene(relative: &str, target_width: u32) -> Canvas {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests")
        .join(relative);
    let data = std::fs::read(&path).unwrap();
    let mut options = usvg::Options::default();
    options
        .fontdb_mut()
        .load_fonts_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/fonts"));
    options.resources_dir = path.parent().map(std::path::Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, &options).unwrap();
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(target_width)
        .unwrap();
    let scale_x = size.width() as f64 / tree.size().width() as f64;
    let scale_y = size.height() as f64 / tree.size().height() as f64;
    let mut canvas = Canvas::new(size.width(), size.height());
    canvas
        .push_svg_with_options(
            &tree,
            SvgOptions {
                transform: peniko::kurbo::Affine::scale_non_uniform(scale_x, scale_y),
                ..SvgOptions::default()
            },
        )
        .unwrap();
    canvas
}

#[test]
fn push_svg_renders_stroke_linejoin_miter_clip() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <path d="M4 26 L16 6 L28 26" fill="none" stroke="#008000"
                      stroke-width="8" stroke-linejoin="miter-clip" stroke-miterlimit="1.5"/>
            </svg>"##,
        Color::TRANSPARENT,
    );
    let bevel = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <path d="M4 26 L16 6 L28 26" fill="none" stroke="#008000"
                      stroke-width="8" stroke-linejoin="bevel" stroke-miterlimit="1.5"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let image = renderer.image();
    let covered = (0..image.height)
        .flat_map(|y| (0..image.width).map(move |x| image.rgba8_at(x, y)))
        .filter(|px| px[1] > 0 && px[3] > 0)
        .count();
    assert!(covered > 100, "covered pixels: {covered}");
    assert_ne!(image.pixels, bevel.image().pixels);
    let top_row_covered = (0..image.width)
        .map(|x| image.rgba8_at(x, 0))
        .filter(|px| px[1] > 0 && px[3] > 0)
        .count();
    assert!(
        top_row_covered < image.width as usize * 3 / 4,
        "miter-clip must not fill the whole top row: {top_row_covered}"
    );
    assert_eq!(renderer.image().rgba8_at(16, 0), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_line_with_default_start_coordinates() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                <path d="M 0 0 L 160 180" stroke="red" stroke-width="4"/>
                <line x2="160" y2="180" stroke="green" stroke-width="4"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    for (x, y) in [(32, 36), (80, 90), (128, 144)] {
        let px = renderer.image().rgba8_at(x, y);
        assert!(
            px[1] > px[0] && px[1] > 0,
            "expected green line coverage at ({x}, {y}), got {px:?}"
        );
    }
    assert_eq!(
        renderer.image().rgba8_at(80, 4),
        [0, 0, 0, 0],
        "the clipped stroke cap must not become a full-width top tile row"
    );
}

#[test]
fn push_svg_renders_line_with_default_y2_coordinate_without_endpoint_tile_fill() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                <path d="M 20 40 L 160 0" stroke="red"/>
                <line x1="20" y1="40" x2="160" stroke="green"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let covered = renderer.image().rgba8_at(90, 20);
    assert!(
        covered[1] > 0,
        "expected green line coverage, got {covered:?}"
    );
    assert_eq!(
        renderer.image().rgba8_at(170, 8),
        [0, 0, 0, 0],
        "line endpoint must not fill the endpoint tile"
    );
}

#[test]
fn push_svg_renders_top_clipped_circle_without_double_top_backdrop() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 200 200">
                <circle cx="100" r="80" fill="green"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert!(
        renderer.image().rgba8_at(260, 8)[1] > 0,
        "expected the top-clipped circle body to cover the right edge tile"
    );
    assert_eq!(
        renderer.image().rgba8_at(271, 8),
        [0, 0, 0, 0],
        "top-clipped circle edge must not fill the whole right edge tile"
    );
}

#[test]
fn push_svg_renders_clip_path_with_multiple_children() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
                <defs>
                    <clipPath id="clip">
                        <rect x="0" y="0" width="4" height="8"/>
                        <rect x="12" y="0" width="4" height="8"/>
                    </clipPath>
                </defs>
                <rect width="16" height="8" fill="#ff0000" clip-path="url(#clip)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 4), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(8, 4), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(14, 4), [255, 0, 0, 255]);
}

#[test]
fn push_svg_renders_clip_path_child_with_nested_clip() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
                <defs>
                    <clipPath id="inner">
                        <rect x="4" y="0" width="8" height="8"/>
                    </clipPath>
                    <clipPath id="outer">
                        <rect width="16" height="8" clip-path="url(#inner)"/>
                    </clipPath>
                </defs>
                <rect width="16" height="8" fill="#ff0000" clip-path="url(#outer)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 4), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(8, 4), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(14, 4), [0, 0, 0, 0]);
}

#[test]
fn push_svg_applies_child_transform_to_nested_clip_path() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
                <defs>
                    <clipPath id="inner">
                        <rect x="4" y="0" width="4" height="8"/>
                    </clipPath>
                    <clipPath id="outer">
                        <rect width="16" height="8" transform="translate(4 0)" clip-path="url(#inner)"/>
                    </clipPath>
                </defs>
                <rect width="16" height="8" fill="#ff0000" clip-path="url(#outer)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(10, 4), [255, 0, 0, 255]);
}

#[test]
fn push_svg_empty_clip_path_clips_everything() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
                <defs>
                    <clipPath id="clip"/>
                </defs>
                <rect width="16" height="8" fill="#ff0000" clip-path="url(#clip)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(8, 4), [0, 0, 0, 0]);
}

#[test]
fn push_svg_keeps_group_opacity_isolated() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">
                <g opacity="0.5">
                    <rect x="16" y="16" width="48" height="48" fill="#ff0000"/>
                    <rect x="32" y="32" width="48" height="48" fill="#ff0000"/>
                </g>
            </svg>"##,
        Color::WHITE,
    );

    let overlap = renderer.image().rgba8_at(40, 40);
    assert_eq!(overlap[0], 255);
    assert!(overlap[1].abs_diff(128) <= 1 && overlap[2].abs_diff(128) <= 1);
}

#[test]
fn push_svg_keeps_opacity_isolated_around_filtered_child() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feOffset in="SourceGraphic" dx="0" dy="0"/>
                    </filter>
                </defs>
                <g opacity="0.5">
                    <rect width="16" height="16" fill="#008000"/>
                    <g filter="url(#f)">
                        <rect width="16" height="16" fill="#0000ff"/>
                    </g>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 128, 128]);
}

#[test]
fn push_svg_keeps_blend_isolated_around_filtered_child() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feOffset in="SourceGraphic" dx="0" dy="0"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#808080"/>
                <g style="mix-blend-mode:multiply">
                    <rect width="16" height="16" fill="#ff0000"/>
                    <g filter="url(#f)">
                        <rect width="16" height="16" fill="#00ff00"/>
                    </g>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 128, 0, 255]);
}

#[test]
fn push_svg_renders_fe_image_href_with_primitive_xy() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <rect id="src" width="8" height="8" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feImage href="#src" x="4" y="4" width="8" height="8"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#f)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(6, 6), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 2), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(13, 13), [0, 0, 0, 0]);
}

#[test]
fn push_svg_fe_image_result_can_feed_composite() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <rect id="src" width="4" height="4" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="4">
                        <feImage href="#src" x="0" y="0" width="4" height="4" result="img"/>
                        <feComposite in="img" in2="SourceAlpha" operator="in"/>
                    </filter>
                </defs>
                <rect x="2" width="4" height="4" fill="#ff0000" filter="url(#f)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(2, 2), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 2), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(4, 2), [0, 0, 0, 0]);
}

#[test]
fn push_svg_fe_image_tracks_filtered_element_transform() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <rect id="src" width="16" height="16" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feImage href="#src"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#f)" transform="scale(0.5)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(6, 6), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(10, 6), [0, 0, 0, 0]);
}

#[test]
fn push_svg_places_external_fe_image_in_transformed_primitive_subregion() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80">
                <defs>
                    <filter id="f" x="0" y="0" width="1" height="1">
                        <feImage x="20" width="20"
                            href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1' height='1'%3E%3Crect width='1' height='1' fill='%23008000'/%3E%3C/svg%3E"/>
                    </filter>
                </defs>
                <rect x="20" y="20" width="40" height="40" fill="#ff0000"
                      filter="url(#f)" transform="rotate(45 40 40)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(16, 26), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(36, 46), [0, 0, 0, 0]);
}

#[test]
fn push_svg_supports_isolated_group_without_opacity_or_blend() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <rect width="16" height="16" fill="#808080"/>
                <g style="isolation:isolate">
                    <rect width="16" height="16" fill="#ff0000" style="mix-blend-mode:multiply"/>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(8, 8), [255, 0, 0, 255]);
}

#[test]
fn push_svg_supports_alpha_mask() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <mask id="m" mask-type="alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="8" height="16">
                        <rect width="16" height="16" fill="#ffffff" fill-opacity="0.5"/>
                    </mask>
                </defs>
                <rect width="16" height="16" fill="#ff0000" mask="url(#m)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(4, 8), [128, 0, 0, 128]);
    assert_eq!(renderer.image().rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn push_svg_supports_luminance_mask() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <rect width="16" height="16" fill="#ff0000"/>
                    </mask>
                </defs>
                <rect width="16" height="16" fill="#00ff00" mask="url(#m)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let px = renderer.image().rgba8_at(8, 8);
    assert!(
        px[1].abs_diff(54) <= 1 && px[3].abs_diff(54) <= 1,
        "got {px:?}"
    );
}

#[test]
fn push_svg_renders_png_image_with_transform() {
    let renderer = render(
        &format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none" image-rendering="optimizeSpeed"/>
                </svg>"##
        ),
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 1), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 1), [0, 0, 255, 255]);
}

#[test]
fn push_svg_smooths_raster_image_by_default() {
    let renderer = render(
        &format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none"/>
                </svg>"##
        ),
        Color::TRANSPARENT,
    );

    let edge = renderer.image().rgba8_at(2, 1);
    assert_eq!(edge[3], 255);
    assert!(
        edge[0] > 0 && edge[2] > 0,
        "expected smoothed red/blue edge, got {edge:?}"
    );
}

#[test]
fn push_svg_uses_nearest_sampling_for_image_rendering_hint() {
    let renderer = render(
        &format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none" style="image-rendering:pixelated"/>
                </svg>"##
        ),
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 1), [0, 0, 255, 255]);
}

#[test]
fn push_svg_decodes_png_image_into_premultiplied_pixels() {
    let renderer = render(
        &format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                    <image href="data:image/png;base64,{TRANSLUCENT_ORANGE_PNG}" width="1" height="1"/>
                </svg>"##
        ),
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(0, 0), [128, 64, 0, 128]);
}

#[test]
fn push_svg_renders_embedded_svg_image() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                <image href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='2' height='1'%3E%3Crect width='1' height='1' fill='%23ff0000'/%3E%3Crect x='1' width='1' height='1' fill='%230000ff'/%3E%3C/svg%3E"
                       width="4" height="2" preserveAspectRatio="none"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 1), [0, 0, 255, 255]);
}

#[test]
fn push_svg_renders_scaled_sliced_embedded_svg_image_top_tile() {
    let renderer = render_with_options(
        r##"<svg width="200" height="200" viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <image x="36" y="3" width="128" height="64"
                       href="data:image/svg+xml,%3Csvg viewBox='0 0 20 20' xmlns='http://www.w3.org/2000/svg'%3E%3Crect fill='%2300f' height='20' rx='5' width='20'/%3E%3Crect fill='none' height='16' rx='4' stroke='%230f0' width='16' x='2' y='2'/%3E%3C/svg%3E"
                       preserveAspectRatio="xMaxYMax slice"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    assert_eq!(renderer.image().rgba8_at(100, 6), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(100, 18), [0, 0, 255, 255]);
}

#[test]
fn svg_image_raster_size_includes_outer_transform_scale() {
    let size = usvg::Size::from_wh(100.0, 100.0).unwrap();

    assert_eq!(svg_image_raster_size(Affine::scale(2.4), size), (240, 240));
}

#[test]
fn push_svg_decodes_common_raster_image_formats() {
    for format in [
        ::image::ImageFormat::Gif,
        ::image::ImageFormat::Jpeg,
        ::image::ImageFormat::WebP,
    ] {
        let raster = decode_encoded_image(&encoded_test_image(format), format, "test image")
            .unwrap_or_else(|err| panic!("{format:?}: {err}"));

        assert_eq!((raster.width, raster.height), (2, 1));
        assert_eq!((raster.pixels[0] >> 24) as u8, 255);
        assert_eq!((raster.pixels[1] >> 24) as u8, 255);
    }
}

#[test]
fn push_svg_renders_linear_gradient() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="4">
                <defs>
                    <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="20" y2="0">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <rect width="20" height="4" fill="url(#g)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let left = renderer.image().rgba8_at(2, 2);
    let right = renderer.image().rgba8_at(18, 2);
    assert!(
        left[0] > left[2],
        "left pixel should be red-biased: {left:?}"
    );
    assert!(
        right[2] > right[0],
        "right pixel should be blue-biased: {right:?}"
    );
}

#[test]
fn push_svg_scales_linear_gradient_paint_with_svg_transform() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg"
                    xmlns:xlink="http://www.w3.org/1999/xlink">
                <defs>
                    <rect id="lg1" y2="1" spreadMethod="reflect" width="50" height="50"/>
                </defs>
                <linearGradient id="lg2" xlink:href="#lg1" x2="0.7">
                    <stop offset="0" stop-color="white"/>
                    <stop offset="1" stop-color="black"/>
                </linearGradient>
                <rect x="20" y="20" width="160" height="160" fill="url(#lg2)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    let left = renderer.image().rgba8_at(32, 150);
    let middle = renderer.image().rgba8_at(150, 150);
    let end = renderer.image().rgba8_at(210, 150);
    assert!(
        left[0] > 245 && left[3] == 255,
        "left side should stay near white after canvas scaling: {left:?}"
    );
    assert!(
        (40..120).contains(&middle[0]) && middle[3] == 255,
        "middle should still be inside the gradient ramp: {middle:?}"
    );
    assert!(
        end[0] < 5 && end[3] == 255,
        "after x2 should clamp to black, not reflect from the rect href: {end:?}"
    );
    assert_eq!(renderer.image().rgba8_at(270, 150)[3], 0);
}

#[test]
fn push_svg_applies_path_transform_to_linear_gradient_paint_server() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <linearGradient id="g" gradientTransform="rotate(30)">
                    <stop offset="0" stop-color="white"/>
                    <stop offset="1" stop-color="black"/>
                </linearGradient>
                <rect x="100" y="40" width="110" height="110"
                      fill="url(#g)" transform="skewX(-30)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    let upper = renderer.image().rgba8_at(120, 90);
    let lower = renderer.image().rgba8_at(150, 150);
    let right = renderer.image().rgba8_at(210, 90);
    assert!(
        (180..230).contains(&upper[0]) && upper[3] == 255,
        "upper gradient sample should be light after skew+rotate: {upper:?}"
    );
    assert!(
        (40..100).contains(&lower[0]) && lower[3] == 255,
        "lower gradient sample should be dark after skew+rotate: {lower:?}"
    );
    assert!(
        right[0] < 120 && right[3] == 255,
        "right edge should not stay too light when path transform is applied: {right:?}"
    );
}

#[test]
fn push_svg_scales_radial_gradient_paint_with_svg_transform() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
                <defs>
                    <radialGradient id="g" gradientUnits="userSpaceOnUse" cx="50" cy="50" r="40">
                        <stop offset="0" stop-color="white"/>
                        <stop offset="1" stop-color="black"/>
                    </radialGradient>
                </defs>
                <rect x="10" y="10" width="80" height="80" fill="url(#g)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(2.0),
            ..Default::default()
        },
        200,
        200,
    );

    let center = renderer.image().rgba8_at(100, 100);
    let edge = renderer.image().rgba8_at(178, 100);
    assert!(
        center[0] > 245 && center[3] == 255,
        "scaled radial gradient center should stay white: {center:?}"
    );
    assert!(
        edge[0] < 20 && edge[3] == 255,
        "scaled radial gradient edge should be near black: {edge:?}"
    );
}

#[test]
fn push_svg_renders_pattern_fill_with_opacity() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">
                        <rect width="2" height="4" fill="#ff0000"/>
                        <rect x="2" width="2" height="4" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="12" height="4" fill="url(#p)" fill-opacity="0.5"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_rgba_close(renderer.image().rgba8_at(1, 2), [128, 0, 0, 128], 1);
    assert_rgba_close(renderer.image().rgba8_at(3, 2), [0, 0, 128, 128], 1);
    assert_rgba_close(renderer.image().rgba8_at(5, 2), [128, 0, 0, 128], 1);
}

#[test]
fn push_svg_applies_pattern_transform_before_repeating() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" patternTransform="translate(2 0)">
                        <rect width="2" height="4" fill="#ff0000"/>
                        <rect x="2" width="2" height="4" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="8" height="4" fill="url(#p)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 2), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(5, 2), [0, 0, 255, 255]);
}

#[test]
fn push_svg_renders_pattern_view_box() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" viewBox="0 0 2 2" preserveAspectRatio="none">
                        <rect width="1" height="2" fill="#ff0000"/>
                        <rect x="1" width="1" height="2" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="8" height="4" fill="url(#p)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 2), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 2), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(5, 2), [255, 0, 0, 255]);
}

#[test]
fn push_svg_renders_fe_gaussian_blur() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">
                <defs>
                    <filter id="blur" x="0" y="0" width="32" height="16" filterUnits="userSpaceOnUse">
                        <feGaussianBlur stdDeviation="2"/>
                    </filter>
                </defs>
                <rect x="8" y="4" width="8" height="8" fill="#ff0000" filter="url(#blur)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let center = renderer.image().rgba8_at(12, 8);
    let spread = renderer.image().rgba8_at(6, 8);
    assert!(
        center[0] > 0 && center[3] > 0,
        "blurred center should retain red coverage: {center:?}"
    );
    assert!(
        spread[0] > 0 && spread[3] > 0,
        "blur should spread outside the original rect: {spread:?}"
    );
}

#[test]
fn push_svg_renders_anisotropic_fe_gaussian_blur() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="3">
                <defs>
                    <filter id="blur" x="0" y="0" width="3" height="3" filterUnits="userSpaceOnUse">
                        <feGaussianBlur stdDeviation="1 0"/>
                    </filter>
                </defs>
                <rect x="1" y="1" width="1" height="1" fill="#ffffff" filter="url(#blur)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert!(renderer.image().rgba8_at(0, 1)[3] > 0);
    assert!(renderer.image().rgba8_at(2, 1)[3] > 0);
    assert_eq!(renderer.image().rgba8_at(1, 0), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_drop_shadow() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
                <defs>
                    <filter id="shadow" x="0" y="0" width="32" height="24" filterUnits="userSpaceOnUse">
                        <feDropShadow dx="8" dy="4" stdDeviation="0" flood-color="#0000ff"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="8" height="8" fill="#00ff00" filter="url(#shadow)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(6, 6), [0, 255, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(16, 10), [0, 0, 255, 255]);
}

#[test]
fn push_svg_renders_fe_offset() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16">
                <defs>
                    <filter id="offset" x="0" y="0" width="24" height="16" filterUnits="userSpaceOnUse">
                        <feOffset dx="6" dy="2"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="4" height="4" fill="#ff0000" filter="url(#offset)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(5, 5), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(11, 7), [255, 0, 0, 255]);
}

#[test]
fn push_svg_fe_offset_preserves_source_outside_viewport_under_transform() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1">
                    <feOffset dx="20" dy="40"/>
                </filter>
                <rect x="20" y="20" width="100" height="100" fill="seagreen"
                      filter="url(#filter1)" transform="skewX(30) translate(-50)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    assert_eq!(renderer.image().rgba8_at(10, 100), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(20, 100), [46, 139, 87, 255]);
    assert_eq!(renderer.image().rgba8_at(220, 190), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_tile_from_unshifted_offset_source_region() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12">
                <defs>
                    <filter id="tile" x="0" y="0" width="24" height="12" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#00ff00" x="1" y="1" width="4" height="4"/>
                        <feOffset dx="2" dy="1"/>
                        <feTile x="0" y="0" width="12" height="8"/>
                    </filter>
                </defs>
                <rect width="12" height="8" fill="#ff0000" filter="url(#tile)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(1, 1), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(3, 2), [0, 255, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(7, 6), [0, 255, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(13, 7), [0, 0, 0, 0]);
}

#[test]
fn push_svg_fe_tile_with_empty_source_region_is_transparent() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="8">
                <defs>
                    <filter id="tile" x="2" y="2" width="8" height="4" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#ff0000" x="20" y="20" width="2" height="2"/>
                        <feOffset dx="1" dy="1"/>
                        <feTile/>
                    </filter>
                </defs>
                <rect x="2" y="2" width="8" height="4" fill="#00ff00" filter="url(#tile)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert!(renderer.image().pixels.iter().all(|px| *px == 0));
}

#[test]
fn push_svg_scales_fe_offset_with_svg_transform() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">
                    <feOffset dx="100"/>
                </filter>
                <rect x="20" y="70" width="60" height="60" fill="green"/>
                <rect x="20" y="70" width="60" height="60" fill="red" filter="url(#filter1)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    assert_eq!(renderer.image().rgba8_at(90, 150), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(150, 150), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(180, 150), [255, 0, 0, 255]);
}

#[test]
fn push_svg_renders_fe_flood() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="flood" x="0" y="0" width="16" height="16" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#00ff00" flood-opacity="0.5"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="4" height="4" fill="#ff0000" filter="url(#flood)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(12, 12), [0, 128, 0, 128]);
}

#[test]
fn push_svg_renders_fe_color_matrix() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="matrix" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feColorMatrix type="matrix" values="
                            0 0 0 0 0
                            0 0 0 0 0
                            1 0 0 0 0
                            0 0 0 1 0"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#matrix)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(4, 4), [0, 0, 255, 255]);
}

#[test]
fn push_svg_renders_fe_color_matrix_luminance_to_alpha() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="alpha" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feColorMatrix type="luminanceToAlpha"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#alpha)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(4, 4), [0, 0, 0, 54]);
}

#[test]
fn push_svg_renders_fe_component_transfer_mixed_types() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="transfer" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feComponentTransfer>
                            <feFuncR type="table" tableValues="0 1 0"/>
                            <feFuncG type="discrete" tableValues="1 0"/>
                            <feFuncB type="gamma" amplitude="1" exponent="2" offset="0"/>
                            <feFuncA type="linear" slope="0.5" intercept="0.25"/>
                        </feComponentTransfer>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#804020" filter="url(#transfer)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(4, 4), [190, 191, 3, 191]);
}

#[test]
fn push_svg_renders_fe_blend_with_input_graph_and_subregion() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="blend" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feBlend in="SourceGraphic" in2="blue" mode="multiply" x="0" y="0" width="4" height="8"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#blend)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 4), [0, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_composite_with_source_alpha() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="mask" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feComposite in="blue" in2="SourceAlpha" operator="in"/>
                    </filter>
                </defs>
                <rect width="4" height="8" fill="#ff0000" filter="url(#mask)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 4), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_composite_arithmetic() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="arith" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feComposite in="SourceGraphic" in2="blue" operator="arithmetic" k1="0" k2="0.5" k3="0.5" k4="0"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#arith)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(4, 4), [128, 0, 128, 255]);
}

#[test]
fn push_svg_scales_filter_graph_primitive_regions() {
    let renderer = render_with_options(
        r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1" color-interpolation-filters="sRGB">
                    <feFlood flood-color="blue"/>
                    <feComposite operator="arithmetic" in2="SourceGraphic"
                        k1="0.1" k2="0.2" k3="0.3" k4="0.4"/>
                </filter>
                <rect x="20" y="20" width="160" height="160" fill="seagreen" filter="url(#filter1)"/>
            </svg>"##,
        Color::TRANSPARENT,
        SvgOptions {
            transform: Affine::scale(1.5),
            ..Default::default()
        },
        300,
        300,
    );

    let inside = renderer.image().rgba8_at(260, 260);
    let flood_only = renderer.image().rgba8_at(10, 10);
    assert!(
        (110..122).contains(&inside[0])
            && (138..150).contains(&inside[1])
            && (182..194).contains(&inside[2])
            && inside[3] == 255,
        "scaled primitive region should not clip the filtered rect: {inside:?}"
    );
    assert_eq!(
        flood_only,
        [102, 102, 153, 153],
        "arithmetic must be evaluated on premultiplied channels before PNG unpremultiply"
    );
}

#[test]
fn push_svg_renders_fe_convolve_matrix() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="1">
                <defs>
                    <filter id="convolve" x="0" y="0" width="3" height="1" filterUnits="userSpaceOnUse">
                        <feConvolveMatrix order="3 1" targetX="1" targetY="0" edgeMode="duplicate" kernelMatrix="1 0 0"/>
                    </filter>
                </defs>
                <g filter="url(#convolve)">
                    <rect x="0" y="0" width="1" height="1" fill="#0a0000"/>
                    <rect x="1" y="0" width="1" height="1" fill="#140000"/>
                    <rect x="2" y="0" width="1" height="1" fill="#280000"/>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(0, 0), [20, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(1, 0), [40, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 0), [40, 0, 0, 255]);
}

#[test]
fn push_svg_renders_fe_diffuse_lighting() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="1">
                <defs>
                    <filter id="diffuse" x="0" y="0" width="3" height="1" filterUnits="userSpaceOnUse">
                        <feDiffuseLighting surfaceScale="1" diffuseConstant="1" lighting-color="#ff0000">
                            <feDistantLight azimuth="180" elevation="0"/>
                        </feDiffuseLighting>
                    </filter>
                </defs>
                <g filter="url(#diffuse)">
                    <rect x="0" width="1" height="1" fill="#000000" fill-opacity="0"/>
                    <rect x="1" width="1" height="1" fill="#000000" fill-opacity="0.5"/>
                    <rect x="2" width="1" height="1" fill="#000000"/>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let center = renderer.image().rgba8_at(1, 0);
    assert!(
        center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
        "expected red diffuse lighting at alpha slope center, got {center:?}"
    );
}

#[test]
fn push_svg_renders_fe_specular_lighting() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                <defs>
                    <filter id="specular" x="0" y="0" width="1" height="1" filterUnits="userSpaceOnUse">
                        <feSpecularLighting in="SourceAlpha" surfaceScale="0" specularConstant="0.5" specularExponent="1" lighting-color="#ff8000">
                            <fePointLight x="0.5" y="0.5" z="1"/>
                        </feSpecularLighting>
                    </filter>
                </defs>
                <rect width="1" height="1" fill="#000000" filter="url(#specular)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(0, 0), [128, 64, 0, 128]);
}

#[test]
fn push_svg_renders_fe_merge_in_graph_order() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="merge" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feMerge x="0" y="0" width="4" height="8">
                            <feMergeNode in="blue"/>
                            <feMergeNode in="SourceGraphic"/>
                        </feMerge>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#merge)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 4), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_morphology_dilate() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="dilate" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feMorphology operator="dilate" radius="1"/>
                    </filter>
                </defs>
                <rect x="3" y="3" width="2" height="2" fill="#ff0000" filter="url(#dilate)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(2, 3), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(5, 4), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn push_svg_renders_fe_morphology_erode() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="erode" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feMorphology operator="erode" radius="1"/>
                    </filter>
                </defs>
                <rect x="2" y="2" width="4" height="4" fill="#ff0000" filter="url(#erode)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(3, 3), [255, 0, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 3), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(6, 3), [0, 0, 0, 0]);
}

#[test]
fn push_svg_preserves_css_filter_function_order() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <rect width="8" height="8" fill="#202020" filter="brightness(200%) invert(100%)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let px = renderer.image().rgba8_at(4, 4);
    assert!(
        px[0].abs_diff(191) <= 1 && px[1].abs_diff(191) <= 1 && px[2].abs_diff(191) <= 1,
        "brightness must run before invert: {px:?}"
    );
}

#[test]
fn push_svg_renders_fe_turbulence_in_primitive_region() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feTurbulence x="4" y="4" width="8" height="8" baseFrequency="0.2" seed="3"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    let mut covered = 0;
    for y in 4..12 {
        for x in 4..12 {
            covered += usize::from(renderer.image().rgba8_at(x, y)[3] > 0);
        }
    }
    assert!(covered > 0);
    assert_eq!(renderer.image().rgba8_at(2, 2), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(13, 13), [0, 0, 0, 0]);
}

#[test]
fn push_svg_fe_turbulence_respects_color_interpolation_filters() {
    let default_linear = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feTurbulence baseFrequency="0.18" seed="4"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );
    let explicit_srgb = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16"
                            color-interpolation-filters="sRGB">
                        <feTurbulence baseFrequency="0.18" seed="4"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_ne!(
        default_linear.image().rgba8_at(8, 8),
        explicit_srgb.image().rgba8_at(8, 8)
    );
    assert_eq!(
        default_linear.image().rgba8_at(8, 8)[3],
        explicit_srgb.image().rgba8_at(8, 8)[3]
    );
}

#[test]
fn push_svg_renders_fe_displacement_map() {
    let renderer = render(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="1">
                <defs>
                    <filter id="displace" x="0" y="0" width="4" height="1"
                            filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
                        <feFlood flood-color="rgb(255,128,0)" flood-opacity="0.5" result="map"/>
                        <feDisplacementMap in="SourceGraphic" in2="map" scale="2"
                                           xChannelSelector="R" yChannelSelector="G"/>
                    </filter>
                </defs>
                <g filter="url(#displace)">
                    <rect x="0" width="1" height="1" fill="#ff0000"/>
                    <rect x="1" width="1" height="1" fill="#00ff00"/>
                    <rect x="2" width="1" height="1" fill="#0000ff"/>
                    <rect x="3" width="1" height="1" fill="#ffff00"/>
                </g>
            </svg>"##,
        Color::TRANSPARENT,
    );

    assert_eq!(renderer.image().rgba8_at(0, 0), [0, 255, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(renderer.image().rgba8_at(2, 0), [255, 255, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(3, 0), [0, 0, 0, 0]);
}

#[test]
fn push_svg_unsupported_features_do_not_modify_scene() {
    let tree = parse(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <linearGradient id="g">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#00ff00"/>
                    </linearGradient>
                </defs>
                <rect width="16" height="16" fill="url(#g)"/>
            </svg>"##,
    );
    let mut canvas = Canvas::new(16, 16);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );

    let err = canvas
        .push_svg_with_options(
            &tree,
            SvgOptions {
                transform: Affine::scale_non_uniform(0.0, 1.0),
                ..SvgOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(err.feature(), "non-invertible gradientTransform");

    let mut renderer = CpuRenderer::new(16, 16, Color::TRANSPARENT);
    renderer.render(&canvas);
    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 255, 255]);
}
