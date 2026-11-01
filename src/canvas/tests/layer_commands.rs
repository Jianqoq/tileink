use super::*;

#[test]
fn fused_stack_producers_encode_only_clip_or_path_group_geometry() {
    use crate::shared::{gpu_coarse::LayerStackRecord, gpu_types::*};
    use peniko::kurbo::Shape;

    let mut canvas = test_scene();
    let rect = Rect::new(0.0, 0.0, 48.0, 48.0);
    canvas.push_clip_layer(
        rect.to_path(0.1),
        Affine::translate((0.25, 0.25)),
        FillRule::EvenOdd,
        0.1,
    );
    canvas.push_clip_sdf_rect_layer(rect, Radius::all(4.0));
    canvas.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.5);
    canvas.push_blend_layer(
        rect.to_path(0.1),
        Affine::IDENTITY,
        0.1,
        Mix::Screen,
        Compose::SrcOver,
    );
    canvas.push_rect(rect, Radius::ZERO, Color::WHITE);
    for _ in 0..4 {
        canvas.pop_layer();
    }
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(plan.layer_stack_data.len(), 4);
    for (entry, (tag, sdf, draw_tag)) in plan.layer_stack_data.iter().zip([
        (GPU_LAYER_CLIP, false, DrawTag::Clip),
        (GPU_LAYER_CLIP, true, DrawTag::Clip),
        (GPU_LAYER_OPACITY, false, DrawTag::Opacity),
        (GPU_LAYER_BLEND, false, DrawTag::Blend),
    ]) {
        let record = LayerStackRecord::from(*entry);
        assert_eq!(record.tag, tag);
        let draw = canvas.draw_records[record.draw as usize];
        assert_eq!(draw.tag, draw_tag);
        assert_eq!(draw.sdf_offset != DrawRecord::NONE, sdf);
        assert_eq!(draw.path_id == DrawRecord::NONE, sdf);
        if sdf {
            assert!(draw.sdf_len >= 9);
            assert!(draw.sdf_offset as usize + draw.sdf_len as usize <= canvas.sdf_blob.len());
        } else {
            assert!((draw.path_id as usize) < canvas.path_records.len());
        }
    }
}

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
    for kind in 0..3 {
        let mut source = test_scene();
        match kind {
            0 => source.push_filter_layer(Filter::Opacity(0.5), region.clone()),
            1 => source.push_backdrop_layer(Filter::Opacity(0.5), region.clone()),
            _ => {
                source.push_backdrop_layer_with_opacity(Filter::Opacity(0.5), region.clone(), 0.25);
            }
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
                } if kind == 0 => (filter, sample_region),
                Layer::Backdrop {
                    filter,
                    sample_region,
                    opacity,
                } if kind != 0 => {
                    assert_eq!(*opacity, (kind == 2).then_some(0.25));
                    (filter, sample_region)
                }
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

#[test]
fn backdrop_opacity_is_a_combined_group_with_isolated_foreground_not_an_outer_opacity() {
    let mut canvas = Canvas::new(80, 60, 1.0);
    canvas.push_backdrop_layer_with_opacity(
        crate::Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: crate::BlurSampling::default(),
        },
        crate::Region::rect(
            peniko::kurbo::Rect::new(10.0, 10.0, 70.0, 50.0),
            crate::Radius::ZERO,
        ),
        0.5,
    );
    canvas.push_rect(
        peniko::kurbo::Rect::new(20.0, 20.0, 30.0, 30.0),
        crate::Radius::ZERO,
        peniko::Color::WHITE,
    );
    assert_eq!(
        canvas.pop_layer(),
        Some(crate::shared::layer::LayerKind::Backdrop)
    );
    let plan = canvas.compile(canvas.current_commands);
    assert!(matches!(
        &plan.ops[0],
        crate::shared::execution::ExecOp::OffscreenLayer {
            layer: crate::shared::execution::OffscreenLayer::Backdrop {
                opacity: Some(0.5),
                ..
            },
            ..
        }
    ));
}

#[test]
fn combined_backdrop_opacity_validates_before_mutating_and_clamps_finite_values() {
    for (input, expected) in [
        (-1.0, 0.0),
        (0.0, 0.0),
        (0.25, 0.25),
        (1.0, 1.0),
        (2.0, 1.0),
    ] {
        let mut canvas = test_scene();
        canvas.push_backdrop_layer_with_opacity(
            Filter::Invert(1.0),
            Region::rect(Rect::new(0.0, 0.0, 40.0, 40.0), Radius::ZERO),
            input,
        );
        canvas.pop_layer();
        assert!(
            matches!(&canvas.command_lists[0].commands[0], Command::Layer {
            layer: Layer::Backdrop { opacity: Some(value), .. }, ..
        } if *value == expected)
        );
    }
    for input in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut canvas = test_scene();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            canvas.push_backdrop_layer_with_opacity(
                Filter::Invert(1.0),
                Region::rect(Rect::new(0.0, 0.0, 40.0, 40.0), Radius::ZERO),
                input,
            );
        }));
        assert!(result.is_err());
        assert!(canvas.layer_stack.is_empty());
        assert!(canvas.command_lists[0].commands.is_empty());
    }
}
