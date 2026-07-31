use std::rc::Rc;

use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene};

use super::retained_bench::{HEIGHT, WIDTH};

pub const BACKGROUND_NODES: usize = 4096;
pub const RATIOS: [f64; 13] = [
    0.005, 0.01, 0.02, 0.05, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 1.00,
];

pub struct Workload {
    background: Rc<Canvas>,
}

impl Workload {
    pub fn new() -> Self {
        Self {
            background: rect_scene(8, 8, Color::from_rgb8(40, 80, 140)),
        }
    }

    pub fn persistent_scene(&self, ratio: f64) -> RetainedScene {
        let root = RetainedNodeId::for_owner(1);
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        for index in 0..BACKGROUND_NODES {
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(index as u64 + 2),
                self.background.clone(),
                Affine::translate(((index % 64) as f64 * 16.0, (index / 64) as f64 * 16.0)),
            );
        }
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            changing_node(),
            clipped_scene(
                rect_scene(WIDTH, HEIGHT, Color::from_rgb8(230, 80, 40)),
                damage_side(ratio),
            ),
            Affine::IDENTITY,
        );
        transaction.commit().unwrap();
        scene
    }

    pub fn mutate_persistent(&self, scene: &mut RetainedScene, ratio: f64, frame: usize) {
        let color = if frame.is_multiple_of(2) {
            Color::from_rgb8(230, 80, 40)
        } else {
            Color::from_rgb8(40, 210, 90)
        };
        let mut transaction = scene.transaction();
        transaction.replace_scene(
            changing_node(),
            clipped_scene(rect_scene(WIDTH, HEIGHT, color), damage_side(ratio)),
        );
        transaction.commit().unwrap();
    }

    pub fn immediate_frames(&self, ratio: f64) -> [Canvas; 2] {
        let side = damage_side(ratio);
        [
            immediate_frame(
                &self.background,
                clipped_scene(
                    rect_scene(WIDTH, HEIGHT, Color::from_rgb8(230, 80, 40)),
                    side,
                ),
            ),
            immediate_frame(
                &self.background,
                clipped_scene(
                    rect_scene(WIDTH, HEIGHT, Color::from_rgb8(40, 210, 90)),
                    side,
                ),
            ),
        ]
    }
}

fn changing_node() -> RetainedNodeId {
    RetainedNodeId::for_owner(BACKGROUND_NODES as u64 + 2)
}

fn damage_side(ratio: f64) -> f64 {
    (ratio.sqrt() * WIDTH as f64).clamp(1.0, WIDTH as f64)
}

fn immediate_frame(background: &Rc<Canvas>, changing: Rc<Canvas>) -> Canvas {
    let mut frame = Canvas::new(WIDTH, HEIGHT, 1.0);
    for index in 0..BACKGROUND_NODES {
        frame.append(
            background,
            ((index % 64) as f64 * 16.0, (index / 64) as f64 * 16.0),
        );
    }
    frame.append(&changing, (0.0, 0.0));
    frame
}

fn clipped_scene(scene: Rc<Canvas>, damage_side: f64) -> Rc<Canvas> {
    let mut clipped = Canvas::new(WIDTH, HEIGHT, 1.0);
    clipped.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, damage_side, damage_side), Radius::ZERO);
    clipped.append(&scene, (0.0, 0.0));
    clipped.pop_layer();
    Rc::new(clipped)
}

fn rect_scene(width: u32, height: u32, color: Color) -> Rc<Canvas> {
    let mut scene = Canvas::new(width, height, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        color,
    );
    Rc::new(scene)
}
