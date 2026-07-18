use super::*;

#[test]
fn wgpu_renderer_draws_text_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);

    renderer.render_with_text(&canvas, &mut font_system, &mut text_context);
    let image = renderer.image();

    assert!(
        image.pixels.iter().any(|pixel| (pixel >> 24) != 0),
        "expected at least one text pixel"
    );
}

#[test]
fn text_clip_enforces_a_non_tile_aligned_pixel_boundary() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("MMMMMMMM", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout_clipped(
        &layout,
        peniko::kurbo::Point::new(8.0, 32.0),
        Rect::new(0.0, 0.0, 37.0, 64.0),
        Color::BLACK,
    );
    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);

    renderer.render_with_text(&canvas, &mut font_system, &mut text_context);
    let image = renderer.image();

    assert!((0..64).any(|y| (0..37).any(|x| image.rgba8_at(x, y)[3] != 0)));
    // Regression: coarse draw bounds select whole 16px tiles. Fine must still enforce a clip that
    // ends inside a tile, otherwise a glyph leaks until that tile's right edge.
    assert!((0..64).all(|y| (37..160).all(|x| image.rgba8_at(x, y)[3] == 0)));
}

#[test]
fn wgpu_renderer_ignores_glyph_runs_without_prepared_text_data() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);

    renderer.render(&canvas);

    assert!(
        renderer
            .image()
            .pixels
            .iter()
            .all(|pixel| (pixel >> 24) == 0),
        "unprepared glyph runs should not sample empty text buffers"
    );
}

#[test]
fn wgpu_renderer_renders_text_directly_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    canvas.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return;
    }
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer direct text storage texture test"),
            size: ::wgpu::Extent3d {
                width: 160,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_with_text_to_wgpu_texture(&canvas, &mut font_system, &mut text_context, &texture)
        .expect("render text directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 160, 64);

    assert!(
        bytes.chunks_exact(4).any(|px| px[3] != 0),
        "expected direct text texture to contain non-transparent pixels"
    );
}

#[test]
fn wgpu_renderer_renders_text_compositing_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    canvas.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);
    renderer.render_with_text(&canvas, &mut font_system, &mut text_context);
    let wgpu_image = renderer.image();

    assert!(
        (0..wgpu_image.height).any(|y| {
            (0..wgpu_image.width).any(|x| wgpu_image.rgba8_at(x, y)[..3] != [236, 238, 242])
        }),
        "expected text compositing to modify the background"
    );
}
