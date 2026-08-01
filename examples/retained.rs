use std::{error::Error, rc::Rc};

use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene, WgpuRenderer};

fn main() -> Result<(), Box<dyn Error>> {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;

    let root = RetainedNodeId::for_owner(1);
    let card = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root)?;

    let mut card_canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
    card_canvas.push_rect(
        Rect::new(48.0, 48.0, 280.0, 180.0),
        Radius::all(24.0),
        Color::from_rgb8(91, 83, 255),
    );
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            Rc::new(card_canvas),
            Affine::IDENTITY,
        )
        .commit()?;

    let mut renderer = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    renderer.image().save("target/retained-before.png")?;

    // The stable node ID makes this a local transform update instead of a scene rebuild.
    scene
        .transaction()
        .set_transform(card, Affine::translate((120.0, 40.0)))
        .commit()?;
    renderer.render_retained(&scene);
    renderer.image().save("target/retained-after.png")?;

    Ok(())
}
