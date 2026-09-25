use super::*;
use crate::{
    render::{
        masks,
        operations::{Masked, Offscreen, OperationAdapter},
    },
    shared::execution::ExecPlan,
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
        crate::render::layers::execute(self, canvas, plan, op, target, cursors).map_err(|error| {
            match error {
                crate::render::layers::LayerError::Adapter(error) => error,
                crate::render::layers::LayerError::UnexpectedClip => {
                    "unexpected native offscreen clip".into()
                }
            }
        })
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
