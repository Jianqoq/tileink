use super::*;
use peniko::{Color, kurbo::Rect};

#[cfg(feature = "wgpu")]
#[path = "tests/wgpu.rs"]
mod wgpu;

fn parse(svg: &str) -> usvg::Tree {
    usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap()
}

#[test]
fn svg_image_raster_size_includes_outer_transform_scale() {
    let size = usvg::Size::from_wh(100.0, 100.0).unwrap();

    assert_eq!(svg_image_raster_size(Affine::scale(2.4), size), (240, 240));
}

fn rejected_gradient_scene() -> (Canvas, Canvas) {
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
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );

    let before = canvas.clone();
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

    (canvas, before)
}

fn assert_unchanged_geometry(actual: &Canvas, expected: &Canvas) {
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&actual.lines),
        bytemuck::cast_slice::<_, u8>(&expected.lines)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&actual.path_records),
        bytemuck::cast_slice::<_, u8>(&expected.path_records)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&actual.draw_records),
        bytemuck::cast_slice::<_, u8>(&expected.draw_records)
    );
    assert_eq!(actual.brush_blob, expected.brush_blob);
    assert_eq!(actual.sdf_blob, expected.sdf_blob);
    assert_eq!(actual.sdf_shadow_blob, expected.sdf_shadow_blob);
    assert_eq!(actual.root_commands, expected.root_commands);
    assert_eq!(actual.command_stack, expected.command_stack);
    let draws = |canvas: &Canvas| {
        canvas
            .command_lists
            .iter()
            .map(|list| {
                list.commands
                    .iter()
                    .map(|command| match command {
                        crate::shared::execution::Command::Draw(draw) => *draw,
                        _ => panic!("rectangle scene must contain only draw commands"),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(draws(actual), draws(expected));
    assert_eq!(actual.scene_images.iter().count(), 0);
    assert!(actual.text_glyphs.is_empty());
    assert!(actual.text_runs.is_empty());
}

#[test]
fn push_svg_unsupported_features_do_not_modify_scene() {
    let (mut canvas, mut expected) = rejected_gradient_scene();
    assert_unchanged_geometry(&canvas, &expected);
    // A rejected SVG must also preserve state used by the next append.
    for scene in [&mut canvas, &mut expected] {
        scene.push_rect(
            Rect::new(2.0, 3.0, 7.0, 8.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
    }
    assert_unchanged_geometry(&canvas, &expected);
}
