use super::*;
use peniko::kurbo::RoundedRect;

fn scene(count: u64, depth: u32) -> RetainedScene {
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(1280, 800, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..count {
        let mut canvas = Canvas::new(1280, 800, 1.0);
        let rect = Rect::new(0.0, 0.0, 128.0, 80.0);
        for level in 0..depth {
            canvas.push_clip_layer(
                RoundedRect::from_rect(rect, 80.0 * (0.1 + f64::from(level % 3) * 0.05))
                    .to_path(0.1),
                Affine::IDENTITY,
                crate::FillRule::NonZero,
                0.1,
            );
        }
        canvas.push_rect(
            rect,
            crate::Radius::ZERO,
            Color::from_rgba8((40 + index) as u8, 110, 190, 211),
        );
        for _ in 0..depth {
            canvas.pop_layer();
        }
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(index + 2),
            std::rc::Rc::new(canvas),
            Affine::translate(((index * 173 % 1152) as f64 + 1.0, (index * 97 % 720) as f64)),
        );
    }
    transaction.commit().unwrap();
    scene
}

#[test]
fn deeper_clip_scene_first_frame_matches_fresh_renderer() {
    if !run_wgpu_tests() {
        return;
    }
    let mut reused = new_test_renderer(1280, 800, Color::TRANSPARENT);
    for (count, depth) in [(384, 1), (8, 1), (8, 2)] {
        let mut previous = scene(count, depth);
        for phase in 0..4 {
            let mut transaction = previous.transaction();
            for index in 0..count {
                transaction.set_transform(
                    RetainedNodeId::for_owner(index + 2),
                    Affine::translate((
                        (index * 173 % 1152) as f64 + f64::from(phase % 2),
                        (index * 97 % 720) as f64,
                    )),
                );
            }
            transaction.commit().unwrap();
            reused.render_retained(&previous);
            let _ = reused.image();
        }
    }
    let deeper = scene(8, 4);
    reused.render_retained(&deeper);
    let actual = reused.image();
    let mut fresh = new_test_renderer(1280, 800, Color::TRANSPARENT);
    fresh.render_retained(&deeper);
    let expected = fresh.image();
    assert_images_near(&actual, &expected, 0, "reprepared nested clips");
}
