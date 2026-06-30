use super::*;
use crate::text::TextLayoutOptions;

#[test]
fn scaled_to_fit_transforms_scene_data_and_rebuilds_scan_metadata() {
    let mut scene = Scene::new(100, 50);
    scene.push_rect(
        Rect::new(10.0, 5.0, 30.0, 15.0),
        Radius::all(2.0),
        Brush::Solid(rgb(255, 0, 0)),
        FillRule::NonZero,
    );
    scene.push_path(
        rect_path(40.0, 10.0, 60.0, 30.0),
        Brush::Solid(rgb(0, 255, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 3.0,
        },
        Region::rect(Rect::new(5.0, 6.0, 25.0, 26.0), Radius::all(1.0)),
    );
    scene.push_rect(
        Rect::new(6.0, 8.0, 12.0, 14.0),
        Radius::ZERO,
        Brush::Solid(rgb(0, 0, 255)),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let original_path_capacity = scene.bd_records[0].segment_capacity;
    let scaled = scene.scaled_to_fit(300, 300);

    assert_eq!(scaled.width, 300);
    assert_eq!(scaled.height, 300);
    match scaled.draw_records[0].sdf {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (30.0, 90.0, 90.0, 120.0));
            assert_eq!(rect.radius.top_left, 6.0);
            assert_eq!(rect.radius.top_right, 6.0);
            assert_eq!(rect.radius.bottom_right, 6.0);
            assert_eq!(rect.radius.bottom_left, 6.0);
        }
        sdf => panic!("expected scaled rect SDF, got {sdf:?}"),
    }

    assert_eq!(
        scaled.path_pixel_bounds(0),
        PixelBounds {
            x0: 120,
            y0: 105,
            x1: 180,
            y1: 165,
        }
    );
    assert!(
        scaled.bd_records[0].segment_capacity > original_path_capacity,
        "scaled scene must rebuild path segment capacity for the larger tile footprint"
    );

    let Command::Layer {
        layer: Layer::Filter {
            filter,
            sample_region,
        },
        ..
    } = &scaled.command_lists[0].commands[2]
    else {
        panic!("expected filter layer command after the two root draws");
    };
    match sample_region {
        Region::Rect { rect, radius } => {
            assert_eq!(*rect, Rect::new(15.0, 93.0, 75.0, 153.0));
            assert_eq!(radius.top_left, 3.0);
            assert_eq!(radius.top_right, 3.0);
            assert_eq!(radius.bottom_right, 3.0);
            assert_eq!(radius.bottom_left, 3.0);
        }
        region => panic!("expected scaled rect region, got {region:?}"),
    }
    match filter {
        Filter::Blur {
            std_dev_x,
            std_dev_y,
        } => {
            assert_eq!(*std_dev_x, 6.0);
            assert_eq!(*std_dev_y, 9.0);
        }
        filter => panic!("expected scaled blur filter, got {filter:?}"),
    }
}

#[test]
#[should_panic(expected = "Scene::scaled_to_fit cannot scale bitmap text")]
fn scaled_to_fit_rejects_bitmap_text_layouts() {
    let mut context = TextContext::new();
    let layout = context.layout(TextLayoutOptions::new("text", 12.0));
    let mut scene = Scene::new(64, 64);
    scene.push_text_layout(&layout, Point::new(4.0, 24.0), Color::BLACK);

    let _ = scene.scaled_to_fit(128, 128);
}
