use super::*;
use crate::wgpu::renderer::ExternalTextureHistoryId;

const INITIAL_SIZE: (u32, u32) = (128, 96);
const RESIZED_SIZE: (u32, u32) = (192, 128);

#[test]
fn portable_external_history_preserves_cached_backdrop_after_resize() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    let root = RetainedNodeId::for_owner(95_000);
    let background = RetainedNodeId::for_owner(95_001);
    let panel = RetainedNodeId::for_owner(95_002);
    let controls = RetainedNodeId::for_owner(95_003);
    let mut scene = RetainedScene::new(INITIAL_SIZE.0, INITIAL_SIZE.1, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            checkerboard(INITIAL_SIZE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            liquid_glass_panel(),
            Affine::translate((24.0, 8.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            controls,
            controls_canvas(26.0),
            Affine::translate((24.0, 8.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = Renderer::new(
        device,
        queue,
        INITIAL_SIZE.0,
        INITIAL_SIZE.1,
        Color::TRANSPARENT,
    );
    let mut incremental_config = incremental.incremental_render_config();
    incremental_config.capture_active_tiles = true;
    incremental.set_incremental_render_config(incremental_config);
    assert!(
        incremental
            .fine
            .as_ref()
            .is_some_and(|fine| fine.uses_portable_textures())
    );
    let initial = external_target(device, INITIAL_SIZE, "initial backdrop history");
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &initial,
            ExternalTextureHistoryId::new(1),
        )
        .unwrap();
    scene
        .transaction()
        .resize(RESIZED_SIZE.0, RESIZED_SIZE.1, 1.0)
        .replace_scene(background, checkerboard(RESIZED_SIZE))
        .set_transform(panel, Affine::translate((80.0, 16.0)))
        .set_transform(controls, Affine::translate((80.0, 16.0)))
        .commit()
        .unwrap();
    let resized = external_target(device, RESIZED_SIZE, "resized backdrop history");
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &resized,
            ExternalTextureHistoryId::new(2),
        )
        .unwrap();
    let before_edit = read_texture_rgba8(
        incremental.device(),
        incremental.queue(),
        &resized,
        RESIZED_SIZE.0,
        RESIZED_SIZE.1,
    );

    // This is the Gallery sequence: the first local control edit after a surface resize. The
    // panel backdrop is unchanged and must be composited back over every cleared dirty tile.
    scene
        .transaction()
        .replace_scene(controls, controls_canvas(58.0))
        .commit()
        .unwrap();
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &resized,
            ExternalTextureHistoryId::new(2),
        )
        .unwrap();
    let stats = incremental.incremental_render_stats();
    assert!(!stats.full_redraw);
    assert!(
        stats.dirty_tiles > 0,
        "the control edit must produce damage"
    );
    assert!(
        stats.reused_offscreen_surfaces > 0,
        "the resize frame's backdrop source history must remain reusable"
    );
    assert_eq!(stats.rerendered_offscreen_surfaces, 0);

    let mut full = Renderer::new(
        device,
        queue,
        RESIZED_SIZE.0,
        RESIZED_SIZE.1,
        Color::TRANSPARENT,
    );
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    let expected = external_target(device, RESIZED_SIZE, "full backdrop reference");
    full.render_retained_to_persistent_wgpu_texture(
        &scene,
        &expected,
        ExternalTextureHistoryId::new(3),
    )
    .unwrap();

    let actual = read_texture_rgba8(
        incremental.device(),
        incremental.queue(),
        &resized,
        RESIZED_SIZE.0,
        RESIZED_SIZE.1,
    );
    let expected = read_texture_rgba8(
        full.device(),
        full.queue(),
        &expected,
        RESIZED_SIZE.0,
        RESIZED_SIZE.1,
    );
    let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
    if let Some(byte) = mismatch {
        let pixel = byte / 4;
        eprintln!(
            "first mismatch ({}, {}): actual={:?}, expected={:?}, before={:?}, stats={:?}",
            pixel % RESIZED_SIZE.0 as usize,
            pixel / RESIZED_SIZE.0 as usize,
            &actual[pixel * 4..pixel * 4 + 4],
            &expected[pixel * 4..pixel * 4 + 4],
            &before_edit[pixel * 4..pixel * 4 + 4],
            incremental.incremental_render_stats(),
        );
    }
    assert_eq!(
        mismatch, None,
        "portable incremental backdrop diverged after resize"
    );
}

pub(super) fn checkerboard(size: (u32, u32)) -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(size.0, size.1, 1.0);
    for y in (0..size.1).step_by(16) {
        for x in (0..size.0).step_by(16) {
            canvas.push_rect(
                Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from((x + 16).min(size.0)),
                    f64::from((y + 16).min(size.1)),
                ),
                crate::Radius::ZERO,
                if (x / 16 + y / 16) % 2 == 0 {
                    Color::from_rgb8(28, 104, 168)
                } else {
                    Color::from_rgb8(218, 172, 38)
                },
            );
        }
    }
    std::rc::Rc::new(canvas)
}

pub(super) fn liquid_glass_panel() -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(96, 96, 1.0);
    let bounds = Rect::new(0.0, 0.0, 96.0, 96.0);
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 10,
            blur_sampling: BlurSampling::FULL_RES,
            tint: Color::TRANSPARENT,
            refraction_factor: 2.0,
            fresnel_factor: 0.0,
            glare_factor: 10.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(bounds, crate::Radius::all(14.0)),
    );
    canvas.push_rect(
        bounds,
        crate::Radius::all(14.0),
        Color::from_rgba8(14, 18, 26, 102),
    );
    canvas.pop_layer();
    std::rc::Rc::new(canvas)
}

fn controls_canvas(thumb_x: f64) -> std::rc::Rc<Canvas> {
    let mut canvas = Canvas::new(96, 96, 1.0);
    canvas.push_rect(
        Rect::new(12.0, 42.0, 84.0, 46.0),
        crate::Radius::all(2.0),
        Color::from_rgba8(240, 244, 248, 140),
    );
    canvas.push_rect(
        Rect::new(thumb_x - 6.0, 36.0, thumb_x + 6.0, 52.0),
        crate::Radius::all(6.0),
        Color::WHITE,
    );
    std::rc::Rc::new(canvas)
}

pub(super) fn external_target(
    device: &::wgpu::Device,
    size: (u32, u32),
    label: &'static str,
) -> ::wgpu::Texture {
    device.create_texture(&::wgpu::TextureDescriptor {
        label: Some(label),
        size: ::wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::COPY_SRC
            | ::wgpu::TextureUsages::COPY_DST
            | ::wgpu::TextureUsages::STORAGE_BINDING
            | ::wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}
