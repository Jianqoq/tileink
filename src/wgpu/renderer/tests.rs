use peniko::{
    Color, Compose, Gradient, Mix,
    kurbo::{Affine, BezPath, Line, Rect, Shape},
};

use super::{Renderer, WgpuRenderTargetId};
use crate::{
    FillRule, Scene, TextContext, TextLayoutOptions,
    cpu::Renderer as CpuRenderer,
    debug::{RenderDebugOptions, RenderOptions},
    render::Render,
    shared::{
        bounds::Bounds,
        brush::Brush,
        layer::{
            filter::{
                BlurSampling, COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE,
                ColorChannel, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
                DisplacementMap, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind,
                LightSource, MorphologyOperator, RectLiquidGlass, SpecularLighting, Turbulence,
                TurbulenceKind,
            },
            mask::{Mask, MaskKind},
            region::Region,
        },
    },
};

const GPU_PTCL_END: u32 = 0;
const GPU_PTCL_SDF: u32 = 9;

#[test]
fn wgpu_renderer_reads_uploaded_cpu_render_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_path(
        Rect::new(2.0, 2.0, 6.0, 6.0).to_path(0.0),
        Color::from_rgb8(220, 64, 72),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_path(
        Rect::new(6.0, 0.0, 8.0, 2.0).to_path(0.0),
        Color::from_rgb8(32, 96, 160),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);

    renderer.render(&scene);
    let image = renderer.image();

    assert!(renderer.scene_buffers.draw_flags_capacity() >= 8);
    assert_eq!(image.rgba8_at(3, 3), [220, 64, 72, 255]);
    assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_reuses_pipelines_when_clear_changes() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_rect(
        Rect::new(2.0, 2.0, 6.0, 6.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 64, 72),
    );
    let mut renderer = Renderer::new_default_device(8, 8, Color::from_rgb8(10, 20, 30));

    renderer.render(&scene);
    assert_eq!(renderer.image().rgba8_at(0, 0), [10, 20, 30, 255]);

    renderer.set_clear_color(Color::from_rgb8(7, 8, 9));
    renderer.render(&scene);
    let image = renderer.image();
    assert_eq!(image.rgba8_at(0, 0), [7, 8, 9, 255]);
    assert_eq!(image.rgba8_at(3, 3), [220, 64, 72, 255]);
}

#[test]
fn wgpu_renderer_profile_includes_cpu_prepare_and_gpu_stages() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        Rect::new(2.0, 2.0, 14.0, 14.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 120, 220),
    );
    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);

    renderer.start_profile();
    renderer.render(&scene);
    let profile = renderer.end_profile().clone();

    assert_eq!(renderer.image().rgba8_at(8, 8), [30, 120, 220, 255]);
    assert!(profile.cpu_time() > std::time::Duration::ZERO);
    assert_profile_has(&profile, "prepare");
    assert_profile_has(&profile, "prepare.compile");
    assert_profile_has(&profile, "scan");
    assert_profile_has(&profile, "coarse");
    assert_profile_has(&profile, "fine");
    if renderer
        .device()
        .features()
        .contains(::wgpu::Features::TIMESTAMP_QUERY)
    {
        assert!(
            profile
                .entries()
                .iter()
                .any(|entry| entry.gpu_duration.is_some()),
            "expected at least one GPU timestamp entry"
        );
    }
}

#[test]
fn wgpu_renderer_uploads_cpu_fallback_to_copy_texture_without_storage() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_path(
        Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
        Color::from_rgb8(10, 20, 30),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer test texture"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::COPY_DST | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_to_wgpu_texture(&scene, &texture)
        .expect("render to wgpu texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

    assert_eq!(&bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)], &[10, 20, 30, 255]);
}

#[test]
fn wgpu_renderer_renders_tile_fine_directly_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_path(
        Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
        Color::from_rgb8(40, 100, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
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
            label: Some("tileink wgpu renderer direct storage texture test"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
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
        .render_to_wgpu_texture(&scene, &texture)
        .expect("render directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

    assert_eq!(
        &bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)],
        &[40, 100, 220, 255]
    );
}

#[test]
fn wgpu_renderer_renders_tile_fine_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(12, 34, 56),
    );
    let mut renderer = Renderer::new_default_device(8, 8, Color::TRANSPARENT);
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
            label: Some("tileink wgpu renderer tile fine storage texture test"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
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
        .render_to_wgpu_texture(&scene, &texture)
        .expect("render tile fine to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

    assert_eq!(&bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)], &[12, 34, 56, 255]);
}

#[test]
fn wgpu_renderer_renders_offscreen_plan_directly_to_storage_texture() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
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
            label: Some("tileink wgpu renderer offscreen storage texture test"),
            size: ::wgpu::Extent3d {
                width: 16,
                height: 16,
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
        .render_to_wgpu_texture(&scene, &texture)
        .expect("render offscreen plan directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 16, 16);

    assert_eq!(
        &bytes[4 * (3 * 16 + 3)..4 * (3 * 16 + 4)],
        &[0, 255, 255, 255]
    );
    assert_eq!(
        &bytes[4 * (3 * 16 + 12)..4 * (3 * 16 + 13)],
        &[0, 255, 0, 255]
    );
}

#[test]
fn wgpu_renderer_debug_capture_uses_native_scan_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 32);
    scene.push_path(
        Rect::new(4.0, 4.0, 20.0, 20.0).to_path(0.1),
        Color::from_rgb8(0, 128, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let options = RenderOptions {
        debug: Some(RenderDebugOptions::new("target/wgpu-debug-capture-test").with_tile((0, 0))),
    };
    let mut renderer = Renderer::new_default_device(32, 32, Color::TRANSPARENT);

    let capture = renderer.render_with_options(&scene, &options);

    assert_eq!(capture.backend, "wgpu");
    assert_eq!(capture.tiles.len(), 4);
    assert!(
        capture
            .tile
            .as_ref()
            .is_some_and(|tile| !tile.paths.is_empty())
    );
    assert!(capture.images.iter().any(|image| image.name == "final.png"));
}

#[test]
fn wgpu_renderer_renders_sdf_primitives_in_fine_pass_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        Rect::new(2.0, 2.0, 14.0, 14.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 120, 220),
    );
    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    assert!(renderer.fine.is_some());

    renderer.render(&scene);
    let image = renderer.image();

    assert_eq!(image.rgba8_at(8, 8), [30, 120, 220, 255]);
    assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_samples_gradient_brush_in_fine_pass_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        &gradient,
    );
    let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
    assert!(renderer.fine.is_some());

    renderer.render(&scene);
    let image = renderer.image();
    let left = image.rgba8_at(2, 8);
    let right = image.rgba8_at(29, 8);

    assert!(left[0] > left[2], "expected red side, got {left:?}");
    assert!(right[2] > right[0], "expected blue side, got {right:?}");
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
}

#[test]
fn wgpu_scan_emits_segments_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_path(
        Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    if renderer.scan_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&scene);
    <Renderer as Render>::scan(&mut renderer, &scene, ());

    let backdrops = renderer.scan.backdrops.read::<i32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.backdrop_len,
    );
    let starts = renderer.scan.tile_segment_range_starts.read::<u32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.backdrop_len,
    );
    let ends = renderer.scan.tile_segment_range_ends.read::<u32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.backdrop_len,
    );
    let segment_bumps = renderer.scan.segment_bumps.read::<u32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.path_count,
    );
    let p0x = renderer.scan.segment_p0x.read::<f32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.segment_capacity,
    );
    let p1y = renderer.scan.segment_p1y.read::<f32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.segment_capacity,
    );

    assert_eq!(backdrops, vec![0]);
    assert_eq!(starts, vec![0]);
    assert_eq!(ends, vec![2]);
    assert_eq!(segment_bumps, vec![2]);
    assert!((p0x[0] - 4.0).abs() < 1e-3);
    assert!((p1y[0] - 16.0).abs() < 1e-6);
}

#[test]
fn wgpu_cumsum_scans_backdrop_rows_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(48, 32);
    scene.push_path(
        Rect::new(0.0, 0.0, 48.0, 32.0).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = Renderer::new_default_device(48, 32, Color::TRANSPARENT);
    if renderer.cumsum.is_none() {
        return;
    }

    renderer.prepare_scene(&scene);
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    renderer.scan.backdrops.upload(
        &device,
        &queue,
        "tileink wgpu cumsum test backdrops",
        &[1, -1, 2, 3, 0, -2],
    );
    <Renderer as Render>::cumsum(&mut renderer, &scene, ());

    assert_eq!(
        renderer.scan.backdrops.read::<i32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.backdrop_len
        ),
        vec![1, 0, 2, 3, 3, 1]
    );
}

#[test]
fn wgpu_coarse_emits_sdf_particles_for_rects_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&scene);
    renderer.coarse_batch(&scene, 0, scene.draw_records.len() as u32, 0, 0);

    assert_eq!(
        renderer.coarse.tile_ptcl_range_starts.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.tile_count
        ),
        vec![0, 2]
    );
    assert_eq!(
        renderer.coarse.tile_ptcl_range_ends.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.tile_count
        ),
        vec![2, 5]
    );
    assert_eq!(
        read_ptcl_tags(&renderer, 5),
        vec![
            GPU_PTCL_SDF,
            GPU_PTCL_END,
            GPU_PTCL_SDF,
            GPU_PTCL_SDF,
            GPU_PTCL_END,
        ]
    );
    assert_eq!(
        renderer.coarse.ptcl_colors.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.coarse_ptcl_capacity
        ),
        vec![0, 0, 0, 1, 0]
    );
}

#[test]
fn wgpu_coarse_tile_draw_bins_respect_batch_range_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&scene);
    renderer.coarse_batch(&scene, 1, 2, 0, 0);

    assert_eq!(
        renderer.coarse.tile_ptcl_range_starts.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.tile_count
        ),
        vec![0, 2]
    );
    assert_eq!(
        renderer.coarse.tile_ptcl_range_ends.read::<u32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.tile_count
        ),
        vec![2, 4]
    );
    assert_eq!(
        read_ptcl_tags(&renderer, 4),
        vec![GPU_PTCL_SDF, GPU_PTCL_END, GPU_PTCL_SDF, GPU_PTCL_END]
    );
    assert_eq!(
        renderer
            .coarse
            .ptcl_colors
            .read::<u32>(renderer.device(), renderer.queue(), 4),
        vec![1, 0, 1, 0]
    );
}

#[test]
fn wgpu_renderer_applies_path_clip_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();
    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);

    renderer.render(&scene);
    let image = renderer.image();

    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_opacity_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_opacity_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();
    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);

    renderer.render(&scene);
    let pixel = renderer.image().rgba8_at(8, 8);

    assert!(
        (126..=129).contains(&pixel[3]),
        "unexpected pixel {pixel:?}"
    );
    assert_eq!(pixel[1], 0);
    assert_eq!(pixel[2], 0);
}

#[test]
fn wgpu_renderer_applies_blend_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut scene = Scene::new(16, 16);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(200, 80, 40));
    scene.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(64, 200, 180));
    scene.pop_layer();

    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    let mut cpu = CpuRenderer::new(16, 16, Color::TRANSPARENT);
    cpu.render(&scene);
    assert_images_near(&renderer.image(), &cpu.image(), 1, "multiply blend layer");
}

#[test]
fn wgpu_renderer_applies_color_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_color_matrix_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_component_transfer_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        table[i] = 0;
        table[COMPONENT_TRANSFER_TABLE_SIZE + i] = if i == 0 { 255 } else { i as u32 };
        table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 0;
        table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = i as u32;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::ComponentTransfer(table),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_convolve_matrix_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(3, 1);
    scene.push_filter_layer(
        Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![1.0, 0.0, 0.0],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::Duplicate,
            preserve_alpha: false,
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(10, 0, 0),
    );
    scene.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 0, 0),
    );
    scene.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(40, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
    assert_eq!(image.rgba8_at(2, 0), [40, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_diffuse_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(3, 1);
    scene.push_filter_layer(
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 1.0,
            diffuse_constant: 1.0,
            lighting_color: [1.0, 0.0, 0.0],
            light_source: LightSource::Distant {
                azimuth: 180.0,
                elevation: 0.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 0),
    );
    scene.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 128),
    );
    scene.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let center = image.rgba8_at(1, 0);

    assert!(
        center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
        "expected red diffuse lighting at alpha slope center, got {center:?}"
    );
}

#[test]
fn wgpu_renderer_applies_specular_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(1, 1);
    scene.push_filter_layer(
        Filter::SpecularLighting(SpecularLighting {
            surface_scale: 0.0,
            specular_constant: 0.5,
            specular_exponent: 1.0,
            lighting_color: [1.0, 0.5, 0.0],
            light_source: LightSource::Point {
                x: 0.5,
                y: 0.5,
                z: 1.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(0, 0), [128, 64, 0, 128]);
}

#[test]
fn wgpu_renderer_executes_filter_graph_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Blend {
                        mode: Mix::Multiply,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(0),
                    input2: Some(FilterInput::SourceAlpha),
                    region: Bounds::new(4, 0, 8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::In,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(1),
                    input2: Some(FilterInput::Primitive(2)),
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::Over,
                    },
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(2, 4), [0, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_applies_filter_graph_displacement_map_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 2);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgba8(255, 0, 0, 128)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 4.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 2.0), crate::Radius::ZERO),
    );
    for x in 0..8 {
        scene.push_rect(
            Rect::new(x as f64, 0.0, x as f64 + 1.0, 2.0),
            crate::Radius::ZERO,
            Color::from_rgb8((x as u8 + 1) * 20, 0, 0),
        );
    }
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let mut cpu = CpuRenderer::new(8, 2, Color::TRANSPARENT);
    cpu.render(&scene);

    assert_images_near(&image, &cpu.image(), 1, "filter graph displacement map");
}

#[test]
fn wgpu_renderer_generates_filter_graph_turbulence_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 24);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(6, 5, 28, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    stitch_tiles: true,
                    linear_rgb: true,
                    ..test_turbulence(TurbulenceKind::FractalNoise, -20, 4)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(4.0, 3.0, 30.0, 22.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(4.0, 3.0, 30.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let mut cpu = CpuRenderer::new(32, 24, Color::TRANSPARENT);
    cpu.render(&scene);

    assert_images_near(&image, &cpu.image(), 1, "filter graph turbulence");
}

#[test]
fn wgpu_renderer_filter_graph_turbulence_uses_surface_origin_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(80, 24);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(40, 4, 72, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    base_frequency_x: 0.09,
                    base_frequency_y: 0.13,
                    tile_x: 40.0,
                    tile_y: 4.0,
                    tile_width: 32.0,
                    tile_height: 16.0,
                    ..test_turbulence(TurbulenceKind::Turbulence, 5, 3)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(40.0, 4.0, 72.0, 20.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(40.0, 4.0, 72.0, 20.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let mut cpu = CpuRenderer::new(80, 24, Color::TRANSPARENT);
    cpu.render(&scene);

    assert_images_near(
        &image,
        &cpu.image(),
        1,
        "filter graph turbulence surface origin",
    );
}

#[test]
fn wgpu_renderer_rasterizes_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut triangle = BezPath::new();
    triangle.move_to((4.0, 4.0));
    triangle.line_to((12.0, 4.0));
    triangle.line_to((4.0, 12.0));
    triangle.close_path();
    let region = Region::path(triangle, Affine::IDENTITY, 0.0);
    let mut scene = Scene::new(16, 16);
    scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    scene.pop_layer();

    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    assert_eq!(
        renderer
            .filter_paths
            .range_starts
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![0]
    );
    assert_eq!(
        renderer
            .filter_paths
            .range_ends
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![3]
    );
    assert_eq!(
        renderer
            .filter_paths
            .p0x
            .read::<i32>(renderer.device(), renderer.queue(), 3),
        vec![1024, 3072, 1024]
    );

    let mask = renderer.acquire_scratch().expect("scratch mask");
    renderer.clear_render_target(mask, 0);
    assert!(renderer.build_region_mask(mask, &region, Some(0), Bounds::new(4, 4, 12, 12)));

    let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
    assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn wgpu_renderer_rasterizes_nonzero_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut path = BezPath::new();
    for _ in 0..2 {
        path.move_to((4.0, 4.0));
        path.line_to((12.0, 4.0));
        path.line_to((4.0, 12.0));
        path.close_path();
    }
    let region = Region::path(path, Affine::IDENTITY, 0.0);
    let mut scene = Scene::new(16, 16);
    scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    scene.pop_layer();

    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let mask = renderer.acquire_scratch().expect("scratch mask");
    renderer.clear_render_target(mask, 0);
    assert!(renderer.build_region_mask(mask, &region, Some(0), Bounds::new(4, 4, 12, 12)));

    let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
    assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn wgpu_renderer_rect_liquid_glass_backdrop_matches_cpu_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(64, 32);
    for x in 0..64 {
        let v = (x * 4) as u8;
        scene.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 32.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, v, v),
        );
    }
    scene.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 6,
            blur_sampling: BlurSampling::downsampled(2),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(16.0, 4.0, 48.0, 28.0), crate::Radius::all(6.0)),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let mut cpu = CpuRenderer::new(64, 32, Color::TRANSPARENT);
    cpu.render(&scene);

    assert_images_near(&image, &cpu.image(), 4, "rect liquid glass backdrop");
}

#[test]
fn wgpu_renderer_merges_filter_graph_inputs_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Merge {
                        inputs: vec![FilterInput::Primitive(0), FilterInput::SourceGraphic],
                    },
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(2, 4), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_tiles_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(6, 4);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(0, 0, 6, 4),
                kind: FilterPrimitiveKind::Tile {
                    source_region: Bounds::new(1, 1, 3, 3),
                },
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 6.0, 4.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(1.0, 1.0, 2.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_rect(
        Rect::new(2.0, 1.0, 3.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    scene.push_rect(
        Rect::new(1.0, 2.0, 2.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    scene.push_rect(
        Rect::new(2.0, 2.0, 3.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 255, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(0, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(3, 1), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_displaces_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(3, 1);
    scene.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::WHITE),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 2.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    scene.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_solid_flood_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Flood {
            brush: Brush::Solid(Color::from_rgba8(20, 40, 80, 128)),
        },
        Region::rect(Rect::new(4.0, 4.0, 12.0, 12.0), crate::Radius::ZERO),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(8, 8), [10, 20, 40, 128]);
    assert_eq!(image.rgba8_at(2, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_samples_gradient_flood_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (15.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Flood {
            brush: Brush::from_gradient(&gradient),
        },
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let left = image.rgba8_at(2, 8);
    let right = image.rgba8_at(13, 8);

    assert!(left[0] > left[2], "expected red side, got {left:?}");
    assert!(right[2] > right[0], "expected blue side, got {right:?}");
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
}

#[test]
fn wgpu_renderer_applies_solid_drop_shadow_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 2.0,
            offset_y: 1.0,
            std_dev: 0.0,
            brush: Brush::Solid(Color::from_rgba8(0, 0, 0, 128)),
        },
        Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(4.0, 4.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(9, 6), [0, 0, 0, 128]);
}

#[test]
fn wgpu_renderer_samples_gradient_drop_shadow_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(32, 32);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 12.0,
            std_dev: 0.0,
            brush: Brush::from_gradient(&gradient),
        },
        Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let left = image.rgba8_at(4, 16);
    let right = image.rgba8_at(27, 16);

    assert!(left[0] > left[2], "expected red shadow side, got {left:?}");
    assert!(
        right[2] > right[0],
        "expected blue shadow side, got {right:?}"
    );
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
    assert_eq!(image.rgba8_at(4, 4), [255, 255, 255, 255]);
}

#[test]
fn wgpu_renderer_isolates_opacity_layer_with_offscreen_child_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut scene = Scene::new(16, 16);
    scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 128, 0));
    scene.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(full, crate::Radius::ZERO),
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    scene.pop_layer();
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(8, 8), [0, 0, 128, 128]);
}

#[test]
fn wgpu_renderer_isolates_blend_layer_with_offscreen_child_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut scene = Scene::new(16, 16);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(full, crate::Radius::ZERO),
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 255, 0));
    scene.pop_layer();
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(4, 8), [0, 128, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [128, 128, 128, 255]);
}

#[test]
fn wgpu_renderer_isolates_plain_layer_with_child_blend_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut scene = Scene::new(16, 16);
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    scene.push_isolate_layer(full.to_path(0.0), Affine::IDENTITY, 0.0);
    scene.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.pop_layer();
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(8, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_alpha_mask_layer_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut mask_scene = Scene::new(16, 16);
    mask_scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgba8(255, 255, 255, 128),
    );

    let mut scene = Scene::new(16, 16);
    scene.push_mask_layer(
        mask_scene,
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(4, 8), [128, 0, 0, 128]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_outer_clip_stack_to_offscreen_output_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_outer_sdf_clip_stack_to_offscreen_output_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO);
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_backdrop_filter_to_existing_target_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(48, 24);
    scene.push_rect(
        Rect::new(0.0, 0.0, 48.0, 24.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(8.0, 4.0, 32.0, 20.0), crate::Radius::ZERO),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(12, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_renders_backdrop_layer_children_after_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(4.0, 4.0, 12.0, 12.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(2, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(24, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_offset_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Offset { dx: 2.0, dy: 1.0 },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 4.0, 4.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(1, 2), [0, 0, 0, 0]);
    assert_eq!(image.rgba8_at(3, 2), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 2), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_blurs_offscreen_children_into_expanded_bounds_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(24.0, 8.0, 40.0, 24.0);
    let mut scene = Scene::new(64, 32);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: Default::default(),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    scene.push_rect(sample, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let expanded = image.rgba8_at(22, 16);

    assert!(
        expanded[0] > 0 && expanded[3] > 0,
        "expected blur outside source rect, got {expanded:?}"
    );
    assert_eq!(image.rgba8_at(12, 16), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_downsampled_blur_matches_cpu_approximation() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(7.0, 5.0, 39.0, 27.0);
    let mut scene = Scene::new(64, 40);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: BlurSampling::downsampled(3),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(10.0, 8.0, 24.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.push_rect(
        Rect::new(22.0, 12.0, 36.0, 25.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 80, 255),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);
    let mut cpu = CpuRenderer::new(64, 40, Color::TRANSPARENT);
    cpu.render(&scene);

    assert_images_near(&image, &cpu.image(), 16, "downsampled blur");
}

#[test]
fn wgpu_renderer_profiles_filter_dispatch_stages_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(7.0, 5.0, 39.0, 27.0);
    let mut scene = Scene::new(64, 40);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: BlurSampling::downsampled(3),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(10.0, 8.0, 24.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let mut renderer = Renderer::new_default_device(64, 40, Color::TRANSPARENT);
    let profile = renderer.render_profiled(&scene);

    assert_profile_has(&profile, "filter.downsample");
    assert_profile_has(&profile, "filter.blur.x");
    assert_profile_has(&profile, "filter.blur.y");
    assert_profile_has(&profile, "filter.upsample");
}

#[test]
fn wgpu_renderer_applies_morphology_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Morphology {
            radius_x: 1.0,
            radius_y: 1.0,
            operator: MorphologyOperator::Dilate,
        },
        Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    scene.push_rect(
        Rect::new(4.0, 4.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    scene.pop_layer();

    let image = render_native_wgpu(&scene);

    assert_eq!(image.rgba8_at(3, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(2, 5), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_spills_deep_clip_stack_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Scene::new(32, 16);
    let depth = crate::shared::gpu_plan::FINE_LOCAL_CLIP_DEPTH + 2;
    for ix in 0..depth {
        scene.push_clip_layer(
            Rect::new(ix as f64 * 4.0, 0.0, 32.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    for _ in 0..depth {
        scene.pop_layer();
    }

    let mut renderer = Renderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    let mut cpu = CpuRenderer::new(32, 16, Color::TRANSPARENT);
    cpu.render(&scene);
    assert_images_near(&renderer.image(), &cpu.image(), 1, "deep clip spill");
}

#[test]
fn wgpu_renderer_spills_deep_opacity_stack_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut scene = Scene::new(16, 16);
    let depth = crate::shared::gpu_plan::FINE_LOCAL_GROUP_DEPTH + 2;
    for _ in 0..depth {
        scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    }
    scene.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    for _ in 0..depth {
        scene.pop_layer();
    }

    let mut renderer = Renderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);

    let mut cpu = CpuRenderer::new(16, 16, Color::TRANSPARENT);
    cpu.render(&scene);
    assert_images_near(&renderer.image(), &cpu.image(), 1, "deep opacity spill");
}

#[test]
fn wgpu_renderer_draws_text_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut scene = Scene::new(160, 64);
    scene.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
    let mut renderer = Renderer::new_default_device(160, 64, Color::TRANSPARENT);

    renderer.render_with_text(&scene, &mut text_context);
    let image = renderer.image();

    assert!(
        image.pixels.iter().any(|pixel| (pixel >> 24) != 0),
        "expected at least one text pixel"
    );
}

#[test]
fn wgpu_renderer_renders_text_directly_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut scene = Scene::new(160, 64);
    scene.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    scene.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = Renderer::new_default_device(160, 64, Color::TRANSPARENT);
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
        .render_with_text_to_wgpu_texture(&scene, &mut text_context, &texture)
        .expect("render text directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 160, 64);

    let mut cpu = CpuRenderer::new(160, 64, Color::TRANSPARENT);
    cpu.render_with_text(&scene, &mut text_context);
    let expected = cpu.image();
    for y in 0..64 {
        for x in 0..160 {
            let i = 4 * (y * 160 + x) as usize;
            let actual = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
            let expected = expected.rgba8_at(x, y);
            for channel in 0..4 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 2,
                    "direct text texture mismatch at ({x}, {y}) channel {channel}: actual {actual:?}, expected {expected:?}"
                );
            }
        }
    }
}

#[test]
fn wgpu_renderer_matches_cpu_text_compositing_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut scene = Scene::new(160, 64);
    scene.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    scene.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = Renderer::new_default_device(160, 64, Color::TRANSPARENT);
    renderer.render_with_text(&scene, &mut text_context);
    let wgpu_image = renderer.image();

    let mut cpu = CpuRenderer::new(160, 64, Color::TRANSPARENT);
    cpu.render_with_text(&scene, &mut text_context);
    assert_images_near(&wgpu_image, &cpu.image(), 2, "linear text compositing");
}

fn run_wgpu_tests() -> bool {
    std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() == Ok("1")
        || std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() == Ok("1")
}

fn test_turbulence(kind: TurbulenceKind, seed: i32, num_octaves: u32) -> Turbulence {
    Turbulence {
        base_frequency_x: 0.07,
        base_frequency_y: 0.11,
        num_octaves,
        seed,
        stitch_tiles: false,
        kind,
        linear_rgb: false,
        transform_x: 0.0,
        transform_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        tile_x: 0.0,
        tile_y: 0.0,
        tile_width: 32.0,
        tile_height: 24.0,
    }
}

fn render_native_wgpu(scene: &Scene) -> crate::shared::image::Image {
    let mut renderer = Renderer::new_default_device(scene.width, scene.height, Color::TRANSPARENT);
    renderer.prepare_scene(scene);
    assert!(
        renderer.render_prepared_tile_plan(scene),
        "expected scene to render through native wgpu path"
    );
    renderer.image()
}

fn assert_images_near(
    actual: &crate::shared::image::Image,
    expected: &crate::shared::image::Image,
    tolerance: u8,
    context: &str,
) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    for y in 0..actual.height {
        for x in 0..actual.width {
            let a = actual.rgba8_at(x, y);
            let e = expected.rgba8_at(x, y);
            for channel in 0..4 {
                let diff = a[channel].abs_diff(e[channel]);
                assert!(
                    diff <= tolerance,
                    "{context} mismatch at ({x}, {y}) channel {channel}: actual {a:?}, expected {e:?}"
                );
            }
        }
    }
}

fn assert_profile_has(profile: &crate::WgpuRenderProfile, name: &'static str) {
    assert!(
        profile.entries().iter().any(|entry| entry.name == name),
        "profile missing {name}; entries: {:?}",
        profile.entries()
    );
}

fn read_render_target_u32(renderer: &Renderer, target: WgpuRenderTargetId, len: usize) -> Vec<u32> {
    let texture = match target {
        WgpuRenderTargetId::Main => renderer.readback_target.texture(),
        WgpuRenderTargetId::Scratch(ix) => renderer.scratch[ix].texture(),
    };
    let bytes = read_texture_rgba8(
        renderer.device(),
        renderer.queue(),
        texture,
        renderer.size.0,
        renderer.size.1,
    );
    let values = bytemuck::cast_slice(&bytes)[..len].to_vec();
    values
}

fn read_ptcl_tags(renderer: &Renderer, len: usize) -> Vec<u32> {
    let words =
        renderer
            .coarse
            .ptcl_tags
            .read::<u32>(renderer.device(), renderer.queue(), len.div_ceil(4));
    (0..len)
        .map(|i| (words[i / 4] >> ((i % 4) * 8)) & 255)
        .collect()
}

fn read_texture_rgba8(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    texture: &::wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let row_bytes = width as ::wgpu::BufferAddress * 4;
    let padded_row_bytes = row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
    let readback = device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu renderer texture readback"),
        size: padded_row_bytes * height as u64,
        usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
        label: Some("tileink wgpu renderer texture readback copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        ::wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: (height > 1).then_some(padded_row_bytes as u32),
                rows_per_image: None,
            },
        },
        ::wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(::wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap()
        });
    device.poll(::wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let view = readback.slice(..).get_mapped_range();
    let mut tight = Vec::with_capacity((row_bytes * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded_row_bytes as usize;
        tight.extend_from_slice(&view[start..start + row_bytes as usize]);
    }
    drop(view);
    readback.unmap();
    tight
}
