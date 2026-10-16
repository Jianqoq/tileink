use super::*;

// Regression: the pure-translation append fast path used to translate local SDF geometry before
// applying an icon's nested view-box scale. That scaled the parent translation and displaced every
// icon during gfx_ui's immediate resize-transition frame.
#[test]
fn translated_append_composes_after_an_icon_like_nested_scale() {
    let mut source = Canvas::new(24, 24, 1.0);
    source.push_rect(
        Rect::new(4.0, 4.0, 20.0, 20.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let mut icon = Canvas::new(24, 24, 1.0);
    icon.push_rect(
        Rect::new(0.0, 0.0, 2.0, 2.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(0, 0, 255)),
    );
    icon.append_transformed(&source, Affine::scale_non_uniform(0.5, 0.5));

    for append in [
        Canvas::append as fn(&mut Canvas, &Canvas, Point),
        |target: &mut Canvas, child: &Canvas, position: Point| {
            target.append_transformed(child, Affine::translate((position.x, position.y)));
        },
    ] {
        let mut target = Canvas::new(256, 64, 1.0);
        append(&mut target, &icon, Point::new(100.0, 8.0));

        let bounds = target
            .draw_records
            .iter()
            .map(|draw| draw.pixel_bounds)
            .map(|bounds| (bounds.x0, bounds.y0, bounds.x1, bounds.y1))
            .collect::<Vec<_>>();
        assert_eq!(bounds, [(100, 8, 102, 10), (102, 10, 110, 18)]);
    }
}

#[test]
fn scene_records_rebuild_after_append() {
    let mut parent = test_scene();
    parent.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    let mut child = test_scene();
    child.push_circle(Circle::new((16.0, 16.0), 8.0), Brush::Solid(rgb(0, 255, 0)));
    parent.append(&child, Point::new(4.0, 5.0));

    assert!(parent.lines.is_empty());
    assert!(parent.path_records.is_empty());
    assert_eq!(
        parent.sdf_blob.len(),
        2 * crate::shared::gpu_sdf::ENCODED_SDF_WORDS
    );
}

#[test]
fn append_fast_path_translates_sdf_without_mutating_child() {
    let mut child = test_scene();
    child.push_rect(
        Rect::new(1.0, 2.0, 5.0, 6.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let original_child_draw = child.draw_records[0];

    let mut parent = test_scene();
    parent.append(&child, Point::new(10.0, 20.0));
    parent.append(&child, Point::new(30.0, 40.0));

    assert_eq!(
        child.draw_records[0].pixel_bounds,
        original_child_draw.pixel_bounds
    );
    assert!(matches!(draw_sdf(&child, 0), Some(Sdf::Rect(_))));
    assert!(original_child_draw.sdf_range().is_some());
    assert_eq!(parent.draw_records.len(), 2);
    assert_eq!(
        parent.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 11,
            y0: 22,
            x1: 15,
            y1: 26,
        }
    );
    assert_eq!(
        parent.draw_records[1].pixel_bounds,
        PixelBounds {
            x0: 31,
            y0: 42,
            x1: 35,
            y1: 46,
        }
    );
}

#[test]
fn append_directly_remaps_root_commands_after_nested_lists() {
    let mut child = test_scene();
    child.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    child.push_opacity_layer(rect_path(0.0, 0.0, 16.0, 16.0), Affine::IDENTITY, 0.1, 0.5);
    child.push_rect(
        Rect::new(8.0, 0.0, 16.0, 8.0),
        Radius::ZERO,
        Brush::Solid(rgb(0, 255, 0)),
    );
    child.pop_layer();
    child.push_rect(
        Rect::new(0.0, 8.0, 8.0, 16.0),
        Radius::ZERO,
        Brush::Solid(rgb(0, 0, 255)),
    );

    let mut parent = test_scene();
    parent.append(&child, Point::new(4.0, 6.0));

    assert_eq!(parent.command_lists.len(), 2);
    assert!(matches!(
        parent.command_lists[0].commands[0],
        Command::Draw(0)
    ));
    assert!(matches!(
        parent.command_lists[0].commands[1],
        Command::Layer {
            layer: Layer::Opacity { draw: 1, .. },
            children: 1,
            ..
        }
    ));
    assert!(matches!(
        parent.command_lists[0].commands[2],
        Command::Draw(3)
    ));
    assert!(matches!(
        parent.command_lists[1].commands[0],
        Command::Draw(2)
    ));
}
