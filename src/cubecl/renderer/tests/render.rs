use super::*;

#[test]
fn render_wgpu_matches_cpu_for_multi_tile_mixed_shapes_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = mixed_shape_scene();
    let mut cpu = CpuRenderer::new(360, 260, Color::WHITE);
    cpu.render(&scene);

    let mut wgpu = WgpuRenderer::new_default_device(360, 260, Color::WHITE);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_debug_capture_reads_back_scan_and_final_image_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = mixed_shape_scene();
    let options = crate::RenderOptions {
        debug: Some(crate::RenderDebugOptions::new("target/cubecl-debug-test").with_tile((4, 4))),
    };
    let mut renderer = WgpuRenderer::new_default_device(360, 260, Color::WHITE);

    let capture = renderer.render_with_options(&scene, &options);

    assert_eq!(capture.backend, "cubecl");
    assert!(
        capture
            .images
            .iter()
            .any(|image| image.name == "tile_final.png")
    );
    assert_eq!(
        capture.tiles.len(),
        scene.width_in_tiles() as usize * scene.height_in_tiles() as usize
    );
    let tile = capture.tile.as_ref().expect("specific tile dump");
    assert!(!tile.paths.is_empty());
    assert_eq!(tile.final_rgba[0], renderer.image().rgba8_at(64, 64));
}
