use super::*;

#[test]
fn sdf_clip_geometry_does_not_change_the_execution_topology_fingerprint() {
    let scene = |rect, radius| {
        let mut canvas = test_scene();
        canvas.push_clip_sdf_rect_layer(rect, radius);
        canvas.push_rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        canvas
    };
    let a = scene(Rect::new(2.0, 3.0, 32.0, 40.0), Radius::ZERO);
    let b = scene(Rect::new(8.0, 9.0, 48.0, 52.0), Radius::all(5.0));
    assert_ne!(a.sdf_blob, b.sdf_blob);
    assert_ne!(
        a.draw_records[0].pixel_bounds,
        b.draw_records[0].pixel_bounds
    );
    assert_eq!(
        a.execution_plan_fingerprint(),
        b.execution_plan_fingerprint()
    );
}

#[test]
fn effect_only_commands_have_no_geometry_dependency_when_remapped() {
    let region = Region::rect(Rect::new(-4.0, 2.0, 40.0, 48.0), Radius::all(3.0));
    for backdrop in [false, true] {
        let mut source = test_scene();
        if backdrop {
            source.push_backdrop_layer(Filter::Opacity(0.5), region.clone());
        } else {
            source.push_filter_layer(Filter::Opacity(0.5), region.clone());
        }
        source.pop_layer();
        assert_eq!(source.draw_count(), 0);
        let fingerprint = source.execution_plan_fingerprint();
        for offset in [0, 17, usize::MAX] {
            let mut remapped = source.clone();
            remapped.command_lists[0].commands[0] =
                Canvas::remap_command(source.command_lists[0].commands[0].clone(), offset, 0);
            let Command::Layer {
                layer, children, ..
            } = &remapped.command_lists[0].commands[0]
            else {
                panic!("effect command");
            };
            assert!(layer.hidden_draw().is_none());
            assert_eq!(*children, 1);
            assert_eq!(remapped.execution_plan_fingerprint(), fingerprint);
            let (filter, sample_region) = match layer {
                Layer::Filter {
                    filter,
                    sample_region,
                } if !backdrop => (filter, sample_region),
                Layer::Backdrop {
                    filter,
                    sample_region,
                } if backdrop => (filter, sample_region),
                _ => panic!("effect kind changed"),
            };
            assert!(matches!(filter, Filter::Opacity(0.5)));
            assert!(
                matches!(sample_region, Region::Rect { rect, .. } if *rect == Rect::new(-4.0, 2.0, 40.0, 48.0))
            );
        }
    }
}

#[test]
fn geometric_layer_draws_remap_through_every_append_route() {
    let rect = Rect::new(1.0, 2.0, 41.0, 42.0);
    for empty in [false, true] {
        for kind in 0..5 {
            let mut child = test_scene();
            child.push_rect(rect, Radius::ZERO, Color::WHITE);
            match kind {
                0 => child.push_clip_layer(
                    rect.to_path(0.1),
                    Affine::translate((0.25, 0.25)),
                    FillRule::NonZero,
                    0.1,
                ),
                1 => child.push_clip_sdf_rect_layer(rect, Radius::all(3.0)),
                2 => child.push_isolate_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1),
                3 => child.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.5),
                _ => child.push_blend_layer(
                    rect.to_path(0.1),
                    Affine::IDENTITY,
                    0.1,
                    peniko::Mix::Screen,
                    peniko::Compose::SrcOver,
                ),
            }
            if !empty {
                child.push_rect(rect, Radius::ZERO, Color::BLACK);
            }
            child.pop_layer();
            let Command::Layer { layer, .. } = &child.command_lists[0].commands[1] else {
                panic!("geometric layer");
            };
            assert_eq!(layer.hidden_draw(), Some(1));
            let source_fingerprint = child.execution_plan_fingerprint();
            for route in 0..3 {
                let mut parent = test_scene();
                for _ in 0..3 {
                    parent.push_rect(rect, Radius::ZERO, Color::WHITE);
                }
                match route {
                    0 => parent.append(&child, Point::new(4.0, 6.0)),
                    1 => parent.append_transformed(&child, Affine::translate((4.0, 6.0))),
                    _ => parent.append_transformed(
                        &child,
                        Affine::translate((4.0, 6.0)) * Affine::scale(0.5),
                    ),
                }
                let Command::Layer {
                    layer, children, ..
                } = &parent.command_lists[0].commands[4]
                else {
                    panic!("appended layer");
                };
                assert_eq!(layer.hidden_draw(), Some(4));
                assert_eq!(*children, 1);
                assert_eq!(parent.draw_count(), 5 + usize::from(!empty));
                assert_eq!(parent.draw_records[4].tag, child.draw_records[1].tag);
                assert_eq!(child.execution_plan_fingerprint(), source_fingerprint);
            }
        }
    }
}
