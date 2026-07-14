use super::*;
use crate::wgpu::renderer::{ExternalTextureHistoryId, tests::backdrop_resize};

const LARGE: (u32, u32) = (256, 192);
const SMALL: (u32, u32) = (128, 96);

#[test]
fn oversized_internal_targets_preserve_filter_pixels_after_shrink() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    let root = RetainedNodeId::for_owner(96_000);
    let background = RetainedNodeId::for_owner(96_001);
    let panel = RetainedNodeId::for_owner(96_002);
    let mut scene = RetainedScene::new(LARGE.0, LARGE.1, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            backdrop_resize::checkerboard(LARGE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            backdrop_resize::liquid_glass_panel(),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut reused = Renderer::new(device, queue, LARGE.0, LARGE.1, Color::TRANSPARENT);
    let large = backdrop_resize::external_target(device, LARGE, "large capacity frame");
    reused
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &large,
            ExternalTextureHistoryId::new(1),
        )
        .unwrap();

    scene
        .transaction()
        .resize(SMALL.0, SMALL.1, 1.0)
        .replace_scene(background, backdrop_resize::checkerboard(SMALL))
        .commit()
        .unwrap();
    let actual_target =
        backdrop_resize::external_target(device, SMALL, "capacity reuse after shrink");
    reused
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &actual_target,
            ExternalTextureHistoryId::new(2),
        )
        .unwrap();

    let mut fresh = Renderer::new(device, queue, SMALL.0, SMALL.1, Color::TRANSPARENT);
    let expected_target =
        backdrop_resize::external_target(device, SMALL, "fresh target after shrink");
    fresh
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &expected_target,
            ExternalTextureHistoryId::new(3),
        )
        .unwrap();

    let actual = read_texture_rgba8(
        reused.device(),
        reused.queue(),
        &actual_target,
        SMALL.0,
        SMALL.1,
    );
    let expected = read_texture_rgba8(
        fresh.device(),
        fresh.queue(),
        &expected_target,
        SMALL.0,
        SMALL.1,
    );
    assert_eq!(
        actual, expected,
        "capacity reuse must not change logical filter sampling"
    );
}
