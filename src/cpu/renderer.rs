use peniko::Color;

use crate::{
    render::Render,
    shared::{
        execution::{ExecNode, ExecPlan},
        image::Image,
        layer::Layer,
    },
};

pub struct Renderer {
    image: Image,
    clear: Color,
    size: (u32, u32),
}

impl Render for Renderer {
    type ScanArgs<'a> = ();

    type CumsumArgs<'a> = ();

    type CoarseArgs<'a> = ();

    type FineArgs<'a> = ();

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, scene: &crate::scene::Scene) {
        let mut iamge = Image::new(self.size.0, self.size.1, self.clear);
        self.execute(scene, &mut iamge);
    }

    fn execute(&mut self, scene: &crate::scene::Scene, args: Self::ExecuteArgs<'_>) {
        self.execute_plan(
            scene,
            &ExecPlan {
                nodes: scene.compile(0),
            },
            args,
        );
    }

    fn scan(&self, scene: &crate::scene::Scene, args: Self::ScanArgs<'_>) {
        todo!()
    }

    fn cumsum(&self, scene: &crate::scene::Scene, args: Self::CumsumArgs<'_>) {
        todo!()
    }

    fn coarse(&self, scene: &crate::scene::Scene, args: Self::CoarseArgs<'_>) {
        todo!()
    }

    fn fine(&self, scene: &crate::scene::Scene, args: Self::FineArgs<'_>) {
        todo!()
    }
}

impl Renderer {
    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        for node in &plan.nodes {
            match node {
                ExecNode::DrawBatch { draws, state: _ } => {
                    self.execute_draw_batch(scene, draws.start, draws.end, target);
                }
                ExecNode::OffscreenLayer { layer, children } => {
                    self.execute_offscreen_layer(scene, layer, children, target);
                }
            }
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        _scene: &crate::scene::Scene,
        layer: &Layer,
        _children: &[ExecNode],
        target: &mut Image,
    ) {
        match layer {
            Layer::Filter { filter: _ } => todo!(),
            Layer::SvgFilter {
                filters: _,
                transform: _,
                max_bounds: _,
            } => todo!(),
            Layer::BackdropFilter {
                filter: _,
                region: _,
            } => todo!(),
            Layer::Mask { mode: _ } => todo!(),
            Layer::Clip(_) | Layer::ClipSdf { .. } => todo!(),
            Layer::Opacity { opacity: _ } | Layer::Blend { blend: _ } => todo!(),
        }
    }

    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        start: usize,
        end: usize,
        target: &mut Image,
    ) {
        if start >= end {
            return;
        }
        let draw_records = &scene.draw_records[start..end];
        self.scan(scene, ());
    }
}
