use std::sync::Arc;

use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{Canvas, RetainedNodeId, RetainedParent, RetainedScene};

use super::retained_bench::{HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub draw_count: usize,
    pub dirty_tile_edge: u32,
}

impl Case {
    pub const ALL: [Self; 4] = [
        Self {
            draw_count: 4,
            dirty_tile_edge: 4,
        },
        Self {
            draw_count: 4,
            dirty_tile_edge: 32,
        },
        Self {
            draw_count: 192,
            dirty_tile_edge: 4,
        },
        Self {
            draw_count: 192,
            dirty_tile_edge: 32,
        },
    ];

    pub fn name(self) -> String {
        format!(
            "draws-{}/dirty-{}x{}",
            self.draw_count, self.dirty_tile_edge, self.dirty_tile_edge
        )
    }
}

pub struct Workload {
    pub scene: RetainedScene,
    moving: RetainedNodeId,
}

impl Workload {
    pub fn new(case: Case) -> Self {
        let root = RetainedNodeId::for_owner(90_000);
        let background = RetainedNodeId::for_owner(90_001);
        let moving = RetainedNodeId::for_owner(90_002);
        let mut background_canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
        for draw in 0..case.draw_count {
            background_canvas.push_rect(
                Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)),
                tileink::Radius::ZERO,
                Color::from_rgba8(
                    32 + (draw as u8).wrapping_mul(37) % 192,
                    48 + (draw as u8).wrapping_mul(53) % 176,
                    64 + (draw as u8).wrapping_mul(71) % 160,
                    16,
                ),
            );
        }
        let dirty_pixels = case.dirty_tile_edge * tileink::TILE_SIZE;
        let mut moving_canvas = Canvas::new(dirty_pixels, dirty_pixels, 1.0);
        moving_canvas.push_rect(
            Rect::new(0.0, 0.0, f64::from(dirty_pixels), f64::from(dirty_pixels)),
            tileink::Radius::ZERO,
            Color::WHITE,
        );

        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                background,
                Arc::new(background_canvas),
                Affine::IDENTITY,
            )
            .insert_scene(
                RetainedParent::content(root),
                None,
                moving,
                Arc::new(moving_canvas),
                Affine::translate((127.0, 128.0)),
            )
            .commit()
            .unwrap();
        Self { scene, moving }
    }

    pub fn moving(&self) -> RetainedNodeId {
        self.moving
    }

    pub fn mutate(moving: RetainedNodeId, scene: &mut RetainedScene, frame: usize) {
        let x = if frame.is_multiple_of(2) {
            128.0
        } else {
            129.0
        };
        scene
            .transaction()
            .set_transform(moving, Affine::translate((x, 128.0)))
            .commit()
            .unwrap();
    }
}
