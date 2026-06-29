use peniko::Color;

use crate::{
    cpu::{buffers::RasterBuffers, pipelines::scan::line_scanned_tile_count},
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
        image::Image,
        offscreen::local_offscreen_scene,
    },
};

pub(in crate::cpu) struct OffscreenSurface {
    pub(in crate::cpu) bounds: Bounds,
    pub(in crate::cpu) image: Image,
    pub(in crate::cpu) scene: crate::scene::Scene,
    pub(in crate::cpu) plan: ExecPlan,
    pub(in crate::cpu) children: Vec<ExecOp>,
    pub(in crate::cpu) buffers: RasterBuffers,
}

impl OffscreenSurface {
    pub(in crate::cpu) fn new(
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        children: &[ExecOp],
        bounds: Bounds,
    ) -> Self {
        let local = local_offscreen_scene(scene, plan, children, bounds, line_scanned_tile_count);
        Self {
            bounds,
            image: Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT),
            scene: local.scene,
            plan: local.plan,
            children: local.children,
            buffers: RasterBuffers::default(),
        }
    }
}
