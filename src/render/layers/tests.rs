use super::*;
use crate::render::{
    retained_surfaces::RetainedSurfaceKind,
    surfaces::test_support::{Adapter, Event, fixture},
};
use crate::{Filter, Radius, Region};
use peniko::{BlendMode, Compose, Mix, kurbo::Rect};

fn run(
    adapter: &mut Adapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    layer: &OffscreenLayer,
) -> Result<(), &'static str> {
    execute(
        adapter,
        canvas,
        plan,
        Offscreen {
            retained_id: None,
            layer,
            outer_stack: 2..4,
            children: &plan.ops[..1],
        },
        RenderTargetId::Main,
        &mut FilterCursors::default(),
    )
}

#[test]
fn group_variants_preserve_opacity_blend_and_children_before_composite() {
    let (canvas, plan) = fixture();
    let multiply = BlendMode::new(Mix::Multiply, Compose::SrcOver);
    for (layer, opacity, blend) in [
        (OffscreenLayer::Isolate { draw: 0 }, None, None),
        (
            OffscreenLayer::Opacity {
                draw: 0,
                opacity: 0.375,
            },
            Some(0.375),
            None,
        ),
        (
            OffscreenLayer::Blend {
                draw: 0,
                mode: multiply,
            },
            None,
            Some(multiply),
        ),
    ] {
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
        run(&mut adapter, &canvas, &plan, &layer).unwrap();
        let composite = adapter
            .events
            .iter()
            .position(|event| *event == Event::Composite(blend))
            .unwrap();
        let draw = adapter
            .events
            .iter()
            .position(|event| matches!(event, Event::Draw(_)))
            .unwrap();
        assert!(draw < composite);
        let actual_opacity: Vec<_> = adapter
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Opacity(value) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(actual_opacity, opacity.into_iter().collect::<Vec<_>>());
        assert!(adapter.targets.iter().all(Option::is_none));
    }
}

#[test]
fn filters_render_local_children_while_backdrops_sample_the_existing_parent_target() {
    let (canvas, plan) = fixture();
    let region = Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO);
    for backdrop in [false, true] {
        let layer = if backdrop {
            OffscreenLayer::Backdrop {
                opacity: None,
                filter: Filter::Invert(1.0),
                sample_region: region.clone(),
            }
        } else {
            OffscreenLayer::Filter {
                filter: Filter::Invert(1.0),
                sample_region: region.clone(),
            }
        };
        let kind = if backdrop {
            RetainedSurfaceKind::Backdrop
        } else {
            RetainedSurfaceKind::Filter
        };
        let mut adapter = Adapter::new(&canvas, kind);
        run(&mut adapter, &canvas, &plan, &layer).unwrap();
        assert_eq!(
            adapter
                .events
                .iter()
                .any(|event| matches!(event, Event::BeginFilter(..))),
            !backdrop
        );
        assert_eq!(
            adapter
                .events
                .iter()
                .any(|event| matches!(event, Event::BackdropPass(..))),
            backdrop
        );
        let draw = adapter
            .events
            .iter()
            .position(|event| matches!(event, Event::Draw(_)))
            .unwrap();
        if backdrop {
            let effect = adapter
                .events
                .iter()
                .position(|event| matches!(event, Event::BackdropPass(..)))
                .unwrap();
            assert!(
                effect < draw,
                "Backdrop foreground is drawn after its parent input is filtered"
            );
        }
        assert!(adapter.targets.iter().all(Option::is_none));
    }
}

#[test]
fn shared_layer_dispatch_preserves_the_underlying_recording_error() {
    let (canvas, plan) = fixture();
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Group);
    adapter.fail = Some("opacity");
    let result = run(
        &mut adapter,
        &canvas,
        &plan,
        &OffscreenLayer::Opacity {
            draw: 0,
            opacity: 0.5,
        },
    );
    assert_eq!(result, Err("opacity"));
    assert!(
        !adapter
            .events
            .iter()
            .any(|event| matches!(event, Event::Composite(_)))
    );
    assert!(adapter.targets.iter().all(Option::is_none));
}
