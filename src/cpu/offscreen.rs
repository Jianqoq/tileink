use peniko::Color;

use crate::{
    cpu::buffers::RasterBuffers,
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
    pub(in crate::cpu) canvas: crate::canvas::Canvas,
    pub(in crate::cpu) plan: ExecPlan,
    pub(in crate::cpu) children: Vec<ExecOp>,
    pub(in crate::cpu) buffers: RasterBuffers,
}

impl OffscreenSurface {
    pub(in crate::cpu) fn new(
        canvas: &crate::canvas::Canvas,
        plan: &ExecPlan,
        children: &[ExecOp],
        bounds: Bounds,
    ) -> Self {
        let local = local_offscreen_scene(canvas, plan, children, bounds);
        Self {
            bounds,
            image: Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT),
            canvas: local.canvas,
            plan: local.plan,
            children: local.children,
            buffers: RasterBuffers::default(),
        }
    }
}
