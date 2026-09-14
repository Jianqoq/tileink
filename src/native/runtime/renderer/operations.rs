use super::*;
use crate::{
    render::{
        groups::{self, Group},
        masks,
        operations::{Masked, Offscreen, OperationAdapter},
    },
    shared::execution::ExecPlan,
    shared::layer::Layer,
};

impl OperationAdapter for Execution<'_> {
    fn offscreen(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Offscreen<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<()> {
        let (opacity, blend) = match op.layer {
            Layer::Isolate => (None, None),
            Layer::Opacity(layer) => (Some(layer.opacity), None),
            Layer::Blend(layer) => (None, Some(layer.mode)),
            Layer::Filter { .. } | Layer::Backdrop { .. } => {
                return Err("native frame filter orchestration is not implemented".into());
            }
            Layer::Clip | Layer::ClipSdf { .. } => {
                return Err("unexpected native offscreen clip".into());
            }
        };
        groups::execute(
            self,
            canvas,
            plan,
            Group {
                retained_id: op.retained_id,
                draw: op.draw,
                outer_stack: op.outer_stack,
                children: op.children,
                opacity,
                blend,
            },
            target,
            cursors,
        )
    }
    fn mask(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: Masked<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<()> {
        masks::execute(self, canvas, plan, op, target, cursors)
    }
}
