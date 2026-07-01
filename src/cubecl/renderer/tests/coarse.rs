use super::*;
use crate::cubecl::pipelines::common::{
    DRAW_FLAG_HAS_SDF, DRAW_FLAG_SOLID_COLOR_FAST_PATH, DRAW_FLAG_SOLID_RECT,
};

fn assert_ptcl_tags(renderer: &WgpuRenderer, expected: &[u32]) {
    let words = renderer.coarse.ptcl_tags.read(renderer.client());
    let tags = (0..expected.len())
        .map(|i| (words[i / 4] >> ((i % 4) * 8)) & 255)
        .collect::<Vec<_>>();
    assert_eq!(tags, expected);
}

#[test]
fn coarse_wgpu_emits_sdf_particles_for_rects_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        blue,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let draw_flags = renderer.scene.draw_flags.read(renderer.client());
    let draw_flags = [draw_flags[0] & 255, (draw_flags[0] >> 8) & 255];
    for flags in draw_flags {
        assert_eq!(flags & DRAW_FLAG_SOLID_RECT, 0);
        assert_eq!(flags & DRAW_FLAG_SOLID_COLOR_FAST_PATH, 0);
        assert_ne!(flags & DRAW_FLAG_HAS_SDF, 0);
    }
    run_default_coarse_stage(&mut renderer, &scene);

    assert_eq!(
        renderer
            .coarse
            .tile_ptcl_range_starts
            .read(renderer.client()),
        vec![0, 2]
    );
    assert_eq!(
        renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
        vec![2, 5]
    );
    assert_ptcl_tags(
        &renderer,
        &[
            CUBE_PTCL_SDF,
            CUBE_PTCL_END,
            CUBE_PTCL_SDF,
            CUBE_PTCL_SDF,
            CUBE_PTCL_END,
        ],
    );
    assert_eq!(
        renderer.coarse.ptcl_colors.read(renderer.client()),
        vec![0, 0, 0, 1, 0]
    );
}

#[test]
fn coarse_wgpu_keeps_particle_order_across_workgroup_draw_chunks_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let draw_count = TILE_WORKGROUP_SIZE as usize + 3;
    let colors = (0..draw_count)
        .map(|i| {
            Color::from_rgb8(
                (i % 251) as u8,
                ((i * 37) % 251) as u8,
                ((i * 73) % 251) as u8,
            )
        })
        .collect::<Vec<_>>();
    let mut scene = Scene::new(16, 16);
    for color in &colors {
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            *color,
            FillRule::NonZero,
        );
    }

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    assert_eq!(renderer.lengths.coarse_ptcl_capacity, draw_count + 1);

    let client = renderer.client.clone();
    renderer
        .scan
        .backdrops
        .replace(&client, &vec![1; draw_count]);
    renderer
        .scan
        .tile_segment_range_starts
        .replace(&client, &vec![0; draw_count]);
    renderer
        .scan
        .tile_segment_range_ends
        .replace(&client, &vec![0; draw_count]);
    run_default_coarse_stage(&mut renderer, &scene);

    let mut expected_tags = vec![CUBE_PTCL_SDF; draw_count];
    expected_tags.push(CUBE_PTCL_END);
    let mut expected_colors = (0..draw_count as u32).collect::<Vec<_>>();
    expected_colors.push(0);

    assert_eq!(
        renderer
            .coarse
            .tile_ptcl_range_starts
            .read(renderer.client()),
        vec![0]
    );
    assert_eq!(
        renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
        vec![draw_count as u32 + 1]
    );
    assert_ptcl_tags(&renderer, &expected_tags);
    assert_eq!(
        renderer.coarse.ptcl_colors.read(renderer.client()),
        expected_colors
    );
}

#[test]
fn coarse_wgpu_keeps_segment_ranges_for_fill_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_path(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::EvenOdd,
        0.0,
    );

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let client = renderer.client.clone();
    renderer.scan.backdrops.replace(&client, &[0]);
    renderer
        .scan
        .tile_segment_range_starts
        .replace(&client, &[2]);
    renderer.scan.tile_segment_range_ends.replace(&client, &[5]);
    run_default_coarse_stage(&mut renderer, &scene);

    assert_eq!(
        renderer
            .coarse
            .tile_ptcl_range_starts
            .read(renderer.client()),
        vec![0]
    );
    assert_eq!(
        renderer.coarse.tile_ptcl_range_ends.read(renderer.client()),
        vec![2]
    );
    assert_ptcl_tags(&renderer, &[CUBE_PTCL_FILL, CUBE_PTCL_END]);
    assert_eq!(
        renderer.coarse.ptcl_segment_starts.read(renderer.client()),
        vec![2, 0]
    );
    assert_eq!(
        renderer.coarse.ptcl_segment_ends.read(renderer.client()),
        vec![5, 0]
    );
    assert_eq!(
        renderer.coarse.ptcl_fill_rules.read(renderer.client()),
        vec![1, 0]
    );
}

#[test]
fn coarse_wgpu_emits_clip_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let client = renderer.client.clone();
    renderer.scan.backdrops.replace(&client, &[1]);
    renderer
        .scan
        .tile_segment_range_starts
        .replace(&client, &[0]);
    renderer.scan.tile_segment_range_ends.replace(&client, &[0]);
    run_default_coarse_stage(&mut renderer, &scene);

    assert_ptcl_tags(&renderer, &[CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_END, 0]);
    assert_eq!(
        renderer.coarse.ptcl_backdrops.read(renderer.client()),
        vec![1, 0, 0]
    );
}

#[test]
fn coarse_wgpu_wraps_draw_batch_with_active_clip_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
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
        red,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);
    run_cumsum_stage(&mut renderer, &scene);

    let plan = renderer.plan.as_ref().unwrap();
    let ExecOp::DrawBatch { draws, layer_stack } = &plan.ops[1] else {
        panic!("expected clipped draw batch");
    };
    let draws = draws.clone();
    let layer_stack = layer_stack.clone();
    renderer.coarse_batch(
        &scene,
        draws.start as u32,
        draws.end as u32,
        layer_stack.start as u32,
        layer_stack.end as u32,
    );

    assert_ptcl_tags(
        &renderer,
        &[
            CUBE_PTCL_BEGIN_CLIP,
            CUBE_PTCL_SDF,
            CUBE_PTCL_END_CLIP,
            CUBE_PTCL_END,
        ],
    );
    assert_eq!(
        renderer.coarse.ptcl_colors.read(renderer.client()),
        vec![0, 1, 0, 0]
    );
}

#[test]
fn coarse_wgpu_wraps_draw_batch_with_active_sdf_clip_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO);
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);

    let plan = renderer.plan.as_ref().unwrap();
    let ExecOp::DrawBatch { draws, layer_stack } = &plan.ops[1] else {
        panic!("expected SDF-clipped draw batch");
    };
    let draws = draws.clone();
    let layer_stack = layer_stack.clone();
    renderer.coarse_batch(
        &scene,
        draws.start as u32,
        draws.end as u32,
        layer_stack.start as u32,
        layer_stack.end as u32,
    );

    assert_ptcl_tags(
        &renderer,
        &[
            CUBE_PTCL_BEGIN_SDF_CLIP,
            CUBE_PTCL_SDF,
            CUBE_PTCL_END_CLIP,
            CUBE_PTCL_END,
        ],
    );
    assert_eq!(
        renderer.coarse.ptcl_colors.read(renderer.client()),
        vec![0, 1, 0, 0]
    );
}

#[test]
fn coarse_wgpu_wraps_draw_batch_with_opacity_and_blend_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_opacity_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);
    run_cumsum_stage(&mut renderer, &scene);

    let plan = renderer.plan.as_ref().unwrap();
    let (draws, layer_stack) = plan
        .ops
        .iter()
        .find_map(|op| match op {
            ExecOp::DrawBatch { draws, layer_stack }
                if layer_stack.end - layer_stack.start == 2 =>
            {
                Some((draws.clone(), layer_stack.clone()))
            }
            _ => None,
        })
        .expect("expected opacity+blend draw batch");
    renderer.coarse_batch(
        &scene,
        draws.start as u32,
        draws.end as u32,
        layer_stack.start as u32,
        layer_stack.end as u32,
    );

    assert_ptcl_tags(
        &renderer,
        &[
            CUBE_PTCL_BEGIN_OPACITY,
            CUBE_PTCL_BEGIN_BLEND,
            CUBE_PTCL_SDF,
            CUBE_PTCL_END_BLEND,
            CUBE_PTCL_END_OPACITY,
            CUBE_PTCL_END,
        ],
    );
    assert_eq!(
        renderer.coarse.ptcl_colors.read(renderer.client()),
        vec![
            128,
            Mix::Multiply as u32 | ((Compose::SrcOver as u32) << 8),
            2,
            0,
            0,
            0
        ]
    );
}
