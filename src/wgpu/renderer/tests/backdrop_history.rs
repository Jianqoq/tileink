use super::*;
use crate::wgpu::renderer::ExternalTextureHistoryId;
use crate::{FullRedrawReason, IncrementalRenderMode};

const SIZE: (u32, u32) = (1180, 760);
const CARD_ORIGIN: (f64, f64) = (331.0, 145.0);

#[test]
fn dirty_threshold_frame_seeds_backdrop_history_for_following_hover_damage() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };

    let root = RetainedNodeId::for_owner(96_000);
    let background = RetainedNodeId::for_owner(96_001);
    let nav_row = RetainedNodeId::for_owner(96_002);
    let card = RetainedNodeId::for_owner(96_003);
    let mut scene = RetainedScene::new(SIZE.0, SIZE.1, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            overview_background(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            nav_row,
            nav_row_canvas(false),
            Affine::translate((18.0, 178.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = Renderer::new(device, queue, SIZE.0, SIZE.1, Color::TRANSPARENT);
    let mut incremental_config = incremental.incremental_render_config();
    incremental_config.capture_active_tiles = true;
    incremental.set_incremental_render_config(incremental_config);
    let target = external_target(device, SIZE, "dirty-threshold backdrop history");
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(10),
        )
        .unwrap();

    // Gallery switches from Overview to Liquid Glass in one transaction. Replacing the page
    // background dirties enough tiles to use the full fast path while the new glass card creates
    // the pre-backdrop source that the next incremental frame needs.
    scene
        .transaction()
        .replace_scene(background, liquid_background())
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            liquid_glass_card(),
            Affine::translate(CARD_ORIGIN),
        )
        .commit()
        .unwrap();
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(10),
        )
        .unwrap();
    assert_eq!(
        incremental.incremental_render_stats().full_redraw_reason,
        Some(FullRedrawReason::DirtyTileThreshold),
        "the regression requires the page-switch frame to take the dirty-threshold fast path"
    );

    // The SVG row ends at x=258. The card starts at x=331, but liquid-glass refraction samples
    // roughly 76 px beyond the card, so this hover invalidates only its left sampling edge.
    scene
        .transaction()
        .replace_scene(nav_row, nav_row_canvas(true))
        .commit()
        .unwrap();
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(10),
        )
        .unwrap();
    let stats = incremental.incremental_render_stats();
    assert!(!stats.full_redraw, "hover must remain an incremental frame");
    assert!(stats.dirty_tiles > 0, "hover must produce retained damage");
    assert!(
        stats.reused_offscreen_surfaces + stats.rerendered_offscreen_surfaces > 0,
        "hover damage must reach the liquid-glass backdrop; stats={stats:?}"
    );

    let mut full = Renderer::new(device, queue, SIZE.0, SIZE.1, Color::TRANSPARENT);
    let mut full_config = full.incremental_render_config();
    full_config.mode = IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(full_config);
    let expected_target = external_target(device, SIZE, "full liquid-glass hover reference");
    full.render_retained_to_persistent_wgpu_texture(
        &scene,
        &expected_target,
        ExternalTextureHistoryId::new(11),
    )
    .unwrap();

    let actual = read_texture_rgba8(device, queue, &target, SIZE.0, SIZE.1);
    let expected = read_texture_rgba8(device, queue, &expected_target, SIZE.0, SIZE.1);
    let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
    if let Some(byte) = mismatch {
        let pixel = byte / 4;
        eprintln!(
            "first mismatch at ({}, {}): actual={:?}, expected={:?}, stats={stats:?}",
            pixel % SIZE.0 as usize,
            pixel / SIZE.0 as usize,
            &actual[pixel * 4..pixel * 4 + 4],
            &expected[pixel * 4..pixel * 4 + 4],
        );
    }
    assert_eq!(
        mismatch, None,
        "incremental hover must match a full frame; divergence appears as the Gallery card's white edge or missing blur"
    );
}

fn overview_background() -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(SIZE.0, SIZE.1, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, f64::from(SIZE.0), f64::from(SIZE.1)),
        crate::Radius::ZERO,
        Color::from_rgb8(18, 23, 32),
    );
    std::rc::Rc::new(canvas)
}

fn liquid_background() -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(SIZE.0, SIZE.1, 1.0);
    for y in (0..SIZE.1).step_by(64) {
        for x in (0..SIZE.0).step_by(64) {
            canvas.push_rect(
                Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from((x + 64).min(SIZE.0)),
                    f64::from((y + 64).min(SIZE.1)),
                ),
                crate::Radius::ZERO,
                if (x / 64 + y / 64) % 2 == 0 {
                    Color::from_rgb8(22, 112, 176)
                } else {
                    Color::from_rgb8(222, 76, 126)
                },
            );
        }
    }
    std::rc::Rc::new(canvas)
}

fn nav_row_canvas(hovered: bool) -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(240, 48, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 240.0, 48.0),
        crate::Radius::all(8.0),
        if hovered {
            Color::from_rgb8(31, 38, 51)
        } else {
            Color::from_rgb8(20, 25, 34)
        },
    );
    std::rc::Rc::new(canvas)
}

fn liquid_glass_card() -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(300, 240, 1.0);
    let bounds = Rect::new(0.0, 0.0, 300.0, 240.0);
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 5,
            blur_sampling: BlurSampling::downsampled(4),
            blur_edge: true,
            tint: Color::TRANSPARENT,
            refraction_thickness: 28.0,
            refraction_factor: 2.5,
            refraction_dispersion: 10.0,
            fresnel_range: 0.0,
            fresnel_hardness: 0.0,
            fresnel_factor: 9.0,
            glare_range: 26.0,
            glare_hardness: 30.0,
            glare_convergence: 51.0,
            glare_opposite_factor: 55.0,
            glare_factor: 100.0,
            glare_angle: 59.0_f32.to_radians(),
        }),
        Region::rect(bounds, crate::Radius::all(34.0)),
    );
    canvas.push_rect(
        bounds,
        crate::Radius::all(34.0),
        Color::from_rgba8(255, 255, 255, 24),
    );
    canvas.pop_layer();
    std::rc::Rc::new(canvas)
}
