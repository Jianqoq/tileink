#[path = "tests/reorder.rs"]
mod reorder;
#[path = "tests/resources.rs"]
mod resources;
#[path = "tests/transactions.rs"]
mod transactions;

use peniko::{Color, kurbo::Shape};

use super::*;
use crate::{Radius, shared::bounds::PixelBounds};

fn update_materializer(
    materializer: &mut PersistentSceneMaterializer,
    scene: &RetainedScene,
) -> bool {
    let changes = scene.changes_since(materializer.version());
    materializer.update(scene, changes)
}

fn leaf(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
    Rc::new(canvas)
}

fn multi_draw_leaf(draws: usize) -> Rc<Canvas> {
    let mut canvas = Canvas::new(256, 256, 1.0);
    for index in 0..draws {
        let x = (index % 16) as f64 * 12.0;
        let y = (index / 16) as f64 * 12.0;
        canvas.push_rect(
            Rect::new(x, y, x + 8.0, y + 8.0),
            Radius::ZERO,
            Color::WHITE,
        );
    }
    Rc::new(canvas)
}

fn empty_leaf() -> Rc<Canvas> {
    Rc::new(Canvas::new(16, 16, 1.0))
}

fn root_retained_commands(materializer: &PersistentSceneMaterializer) -> Vec<RetainedNodeId> {
    materializer.canvas.command_lists[0]
        .commands
        .iter()
        .map(|command| match command {
            Command::MaterializedRetainedScene { id, .. } => *id,
            Command::Layer {
                retained: Some(key),
                ..
            }
            | Command::MaskLayer {
                retained: Some(key),
                ..
            } => key.id,
            _ => unreachable!("persistent root only contains retained commands"),
        })
        .collect()
}

fn backdrop_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    canvas.pop_layer();
    Rc::new(canvas)
}

fn path_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        Rect::new(4.0, 6.0, 20.0, 18.0).to_path(0.1),
        Color::WHITE,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    Rc::new(canvas)
}

#[path = "tests/batch_classification.rs"]
mod batch_classification;

#[path = "tests/fragment_membership.rs"]
mod fragment_membership;

#[path = "tests/root_layer_classification.rs"]
mod root_layer_classification;

#[path = "tests/placement.rs"]
mod placement;

#[path = "tests/rollback.rs"]
mod rollback;

#[path = "tests/validation.rs"]
mod validation;

#[path = "tests/surface.rs"]
mod surface;

#[path = "tests/deferred_surface.rs"]
mod deferred_surface;

#[path = "tests/storage.rs"]
mod storage;

#[path = "tests/painter.rs"]
mod painter;

#[path = "tests/dependencies.rs"]
mod dependencies;

#[path = "tests/journal.rs"]
mod journal;

#[path = "tests/topology.rs"]
mod topology;

#[path = "tests/text_storage.rs"]
mod text_storage;
