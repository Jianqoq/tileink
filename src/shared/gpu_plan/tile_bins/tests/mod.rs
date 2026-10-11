use super::*;
use crate::shared::gpu_plan::{GpuBufferLengths, GpuLengthOverrides};
use crate::{Canvas, RetainedNodeId, canvas::SceneBufferChanges};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

mod build;
mod membership;
mod order_queries;
mod storage;
mod updates;

// Exercise the actual retained producer so order, membership and counts are coherent.
fn materialize(canvas: Canvas) -> Canvas {
    let root = RetainedNodeId::for_owner(70_000);
    let (width, height) = canvas.logical_size();
    let mut scene = crate::RetainedScene::new(width, height, canvas.scale_factor(), root).unwrap();
    scene
        .transaction()
        .insert_scene(
            crate::RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(70_001),
            std::rc::Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    (*crate::retained_scene::PersistentSceneMaterializer::new(&scene).canvas()).clone()
}
