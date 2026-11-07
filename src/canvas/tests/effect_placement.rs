//! Effect metadata must move in the same coordinate frame as cached geometry.
use super::*;
use crate::shared::layer::filter;
use peniko::kurbo::Shape;

fn source(dpi: f32, path: bool, kind: usize) -> Canvas {
    let mut canvas = Canvas::new(64, 64, dpi);
    let rect = Rect::new(4., 6., 36., 30.);
    canvas.push_clip_layer(rect.to_path(0.1), Affine::IDENTITY, FillRule::NonZero, 0.1);
    let region = if path {
        Region::path(rect.to_path(0.1), Affine::IDENTITY, 0.1)
    } else {
        Region::rect(rect, Radius::ZERO)
    };
    match kind {
        0 => canvas.push_filter_layer(Filter::Opacity(0.5), region),
        1 => canvas.push_backdrop_layer(Filter::Opacity(0.5), region),
        _ => {
            let mut mask = Canvas::new(64, 64, dpi);
            mask.push_rect(rect, Radius::ZERO, Color::WHITE);
            canvas.push_mask_layer(
                mask,
                Mask {
                    region,
                    kind: crate::MaskKind::Alpha,
                },
            );
        }
    }
    canvas.push_rect(rect, Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    canvas.pop_layer();
    canvas
}
fn check(canvas: &Canvas, dpi: f32, transform: Affine) {
    let region = canvas
        .command_lists
        .iter()
        .flat_map(|list| &list.commands)
        .find_map(|command| match command {
            Command::Layer {
                layer: Layer::Filter { sample_region, .. } | Layer::Backdrop { sample_region, .. },
                ..
            } => Some(sample_region),
            Command::MaskLayer { layer, .. } => Some(&layer.region),
            _ => None,
        })
        .unwrap();
    let bounds = filter::region_bounds(region);
    let device = Affine::scale(f64::from(dpi));
    let expected = (device * transform).transform_rect_bbox(Rect::new(4., 6., 36., 30.));
    for (actual, expected) in [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
        .into_iter()
        .map(f64::from)
        .zip([expected.x0, expected.y0, expected.x1, expected.y1])
    {
        assert!(
            (actual - expected).abs() <= 1.0001,
            "effect region {bounds:?} does not follow placement {transform:?}; expected {expected}"
        );
    }
    let clip = canvas
        .draw_records
        .iter()
        .find(|draw| draw.tag == DrawTag::Clip)
        .unwrap()
        .pixel_bounds;
    assert!(
        (bounds.x0 - clip.x0).abs() <= 1
            && (bounds.y0 - clip.y0).abs() <= 1
            && (bounds.x1 - clip.x1).abs() <= 1
            && (bounds.y1 - clip.y1).abs() <= 1,
        "effect region and clip geometry disagree: {bounds:?} versus {clip:?}"
    );
}
#[test]
fn appended_effect_regions_follow_scale_rotation_and_nested_placement() {
    for dpi in [1., 1.25, 1.5, 2.] {
        for path in [false, true] {
            for kind in 0..3 {
                let source = source(dpi, path, kind);
                for transform in [
                    Affine::translate((60., 72.)) * Affine::scale(0.99),
                    Affine::translate((380., 72.)) * Affine::scale(0.99),
                    Affine::translate((70., 20.)) * Affine::rotate(0.25),
                    Affine::translate((-2., 17.)) * Affine::skew(0.15, -0.1),
                ] {
                    let mut canvas = Canvas::new(780, 360, dpi);
                    canvas.append_transformed(&source, transform);
                    check(&canvas, dpi, transform);
                    let translation = Affine::translate((11., 9.));
                    let mut nested = Canvas::new(800, 400, dpi);
                    nested.append_transformed(&canvas, translation);
                    check(&nested, dpi, translation * transform);
                }
            }
        }
    }
}
#[test]
fn retained_effect_regions_recompute_from_original_metadata_across_reversals() {
    for dpi in [1., 1.25, 1.5, 2.] {
        for path in [false, true] {
            for kind in 0..3 {
                let mut canvas = source(dpi, path, kind);
                let initial = Affine::translate((60., 72.)) * Affine::scale(0.99);
                for transform in [
                    initial,
                    Affine::translate((380., 72.)) * Affine::scale(0.98),
                    Affine::translate((90., 30.)) * Affine::rotate(0.25),
                    Affine::IDENTITY,
                    initial,
                    Affine::translate((380., 72.)) * Affine::scale(0.99),
                ] {
                    canvas.set_retained_transform(transform);
                    check(&canvas, dpi, transform);
                }
            }
        }
    }
}

#[test]
fn retained_effect_sources_are_independent_on_clone_and_cleared_on_rerecord() {
    let first = Affine::translate((60., 72.)) * Affine::scale(0.99);
    let second = Affine::translate((380., 72.)) * Affine::rotate(0.25);
    let mut canvas = source(1., false, 0);
    canvas.set_retained_transform(first);
    let mut cloned = canvas.clone();
    cloned.set_retained_transform(second);
    check(&canvas, 1., first);
    check(&cloned, 1., second);

    canvas.reset();
    // Re-recording changes command-list indices and layer kinds. Old descriptor snapshots
    // must not replace the new draw or reintroduce the previous filter (root-cause guard).
    canvas.push_rect(Rect::new(0., 0., 2., 2.), Radius::ZERO, Color::WHITE);
    canvas.append(&source(1., true, 2), Point::ZERO);
    canvas.set_retained_transform(second);
    check(&canvas, 1., second);
    assert!(matches!(
        canvas.command_lists[0].commands[0],
        Command::Draw(_)
    ));
    assert!(
        canvas
            .command_lists
            .iter()
            .flat_map(|list| &list.commands)
            .any(|command| matches!(command, Command::MaskLayer { .. }))
    );
    assert!(
        !canvas
            .command_lists
            .iter()
            .flat_map(|list| &list.commands)
            .any(|command| matches!(
                command,
                Command::Layer {
                    layer: Layer::Filter { .. },
                    ..
                }
            ))
    );
}

#[test]
fn rounded_mask_affines_preserve_the_shape_instead_of_its_bounding_box() {
    let rect = Rect::new(4., 6., 36., 30.);
    let radius = Radius::all(7.);
    for transform in [
        Affine::rotate(0.25),
        Affine::skew(0.3, 0.1),
        Affine::scale_non_uniform(2., 0.5),
    ] {
        let mut canvas = Canvas::new(64, 64, 1.);
        canvas.push_filter_layer(Filter::Opacity(0.5), Region::rect(rect, radius));
        canvas.pop_layer();
        canvas.set_retained_transform(transform);
        let Command::Layer {
            layer:
                Layer::Filter {
                    sample_region:
                        Region::Path {
                            path,
                            transform: placed,
                            ..
                        },
                    ..
                },
            ..
        } = &canvas.command_lists[0].commands[0]
        else {
            panic!("a rotated or elliptically scaled rounded region needs path geometry");
        };
        assert_eq!(
            *placed,
            Affine::new(transform.as_coeffs().map(|value| f64::from(value as f32)))
        );
        assert_eq!(path.winding(rect.center()), 1);
        assert_eq!(
            path.winding(Point::new(4.1, 6.1)),
            0,
            "a rounded corner must stay excluded"
        );
    }
}
