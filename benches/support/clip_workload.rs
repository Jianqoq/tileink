//! Controlled clip sweeps. Area means each outer clip's bounding box / viewport.
//! Nested masks share that box, keeping depth independent of area and draw count.
use peniko::Color;
use peniko::kurbo::{Affine, Rect, RoundedRect, Shape};
use std::rc::Rc;
use tileink::{Canvas, FillRule, Radius, RetainedNodeId, RetainedParent, RetainedScene};

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub name: &'static str,
    pub count: u64,
    pub depth: u32,
    pub width: u32,
    pub height: u32,
}

macro_rules! case {
    ($name:literal, $count:literal, $depth:literal, $w:literal, $h:literal) => {
        Case {
            name: $name,
            count: $count,
            depth: $depth,
            width: $w,
            height: $h,
        }
    };
}

pub const CASES: [Case; 16] = [
    case!("clip-count-1", 1, 1, 128, 80),
    case!("clip-count-8", 8, 1, 128, 80),
    case!("clip-count-32", 32, 1, 128, 80),
    case!("clip-count-128", 128, 1, 128, 80),
    case!("clip-count-384", 384, 1, 128, 80),
    case!("clip-depth-2", 8, 2, 128, 80),
    case!("clip-depth-4", 8, 4, 128, 80),
    case!("clip-depth-8", 8, 8, 128, 80),
    case!("clip-depth-16", 8, 16, 128, 80),
    case!("clip-depth-32", 8, 32, 128, 80),
    case!("clip-area-0.094pct", 8, 1, 40, 24),
    case!("clip-area-10pct", 8, 1, 400, 256),
    case!("clip-area-50.4pct", 8, 1, 896, 576),
    case!("clip-area-100pct", 8, 1, 1280, 800),
    case!("clip-mixed-32x8", 32, 8, 128, 80),
    case!("clip-mixed-8x8-large", 8, 8, 400, 256),
];

impl Case {
    pub fn transform(self, index: u64, phase: u32) -> Affine {
        // Fixed deterministic placement; every node moves one pixel each frame.
        // Full-size masks extend one pixel past the right edge on odd phases.
        let x = (index * 173) % u64::from((1280 - self.width).max(1));
        let y = (index * 97) % u64::from((800 - self.height).max(1));
        Affine::translate((x as f64 + f64::from(phase % 2), y as f64))
    }

    pub fn canvas(self, index: u64) -> Canvas {
        let mut canvas = Canvas::new(1280, 800, 1.0);
        let rect = Rect::new(0.0, 0.0, f64::from(self.width), f64::from(self.height));
        for level in 0..self.depth {
            // Same bounds, varying corners: measure actual nested masks without
            // shrinking the dispatch region as depth grows.
            let radius = f64::from(self.height) * (0.1 + f64::from(level % 3) * 0.05);
            canvas.push_clip_layer(
                RoundedRect::from_rect(rect, radius).to_path(0.1),
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
        }
        canvas.push_rect(
            rect,
            Radius::ZERO,
            Color::from_rgba8((40 + index % 160) as u8, 110, 190, 211),
        );
        for _ in 0..self.depth {
            canvas.pop_layer();
        }
        canvas
    }

    pub fn scene(self) -> RetainedScene {
        let root = RetainedNodeId::for_owner(1);
        let mut scene = RetainedScene::new(1280, 800, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        for index in 0..self.count {
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(index + 2),
                Rc::new(self.canvas(index)),
                self.transform(index, 0),
            );
        }
        transaction.commit().unwrap();
        scene
    }
}
