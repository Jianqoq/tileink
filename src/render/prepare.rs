//! Per-renderer CPU preparation shared by every GPU adapter.
//!
//! Keep the outer scene's metadata separate from the adapter's temporary local
//! execution plan. Patched descriptor values must consume Canvas's current plan
//! while retaining topology metadata; reusing the old plan would execute stale bounds.

use std::rc::Rc;

use crate::shared::execution::{ExecPlan, ROOT_COMMAND_LIST_ID};
use crate::shared::gpu_plan::plan_stack_depths;
use crate::text::{PreparedTextChanges, PreparedTextData};
use crate::{Canvas, TextContext, TextFontSystem};

#[derive(Default)]
pub(crate) struct ScenePreparation {
    metadata: Option<PlanMetadata>,
}

#[derive(Clone, Copy)]
struct PlanMetadata {
    fingerprint: u64,
    stack_depths: (usize, usize),
    has_programs: bool,
}

pub(crate) struct PreparedPlan {
    pub(crate) plan: Rc<ExecPlan>,
    pub(crate) reused_metadata: bool,
    pub(crate) stack_depths: (usize, usize),
    pub(crate) has_programs: bool,
}

impl PreparedPlan {
    pub(crate) fn new(canvas: &Canvas, plan: Rc<ExecPlan>) -> Self {
        Self {
            stack_depths: plan_stack_depths(&plan),
            has_programs: has_program_brushes(canvas),
            plan,
            reused_metadata: false,
        }
    }
}

fn has_program_brushes(canvas: &Canvas) -> bool {
    canvas.draw_records.iter().any(|draw| {
        canvas.brush_blob.get(draw.brush_offset as usize)
            == Some(&crate::shared::gpu_layout::brush::GPU_BRUSH_PROGRAM)
    })
}

impl ScenePreparation {
    pub(crate) fn prepare_plan(
        &mut self,
        canvas: &Canvas,
        cached_plan: &mut Option<Rc<ExecPlan>>,
    ) -> PreparedPlan {
        let fingerprint = canvas.execution_plan_fingerprint();
        let changes = canvas.buffer_changes.as_ref();
        let exact_reuse = cached_plan.is_some()
            && self
                .metadata
                .is_some_and(|metadata| metadata.fingerprint == fingerprint);
        // Installing a retained plan does not initialize this renderer's metadata.
        // A structural hint only preserves metadata previously prepared here.
        let reusable = self.metadata.filter(|_| {
            cached_plan.is_some()
                && (exact_reuse
                    || changes.is_some_and(|changes| {
                        changes.plan_structure_reused || changes.plan_values_patched
                    }))
        });
        let plan = if exact_reuse {
            cached_plan.take().expect("cached execution plan")
        } else {
            canvas.compile_shared(ROOT_COMMAND_LIST_ID)
        };
        let prepared = if let Some(metadata) = reusable {
            PreparedPlan {
                plan,
                reused_metadata: true,
                stack_depths: metadata.stack_depths,
                // Topology fingerprints exclude paint. Only an explicit unchanged
                // retained draw/brush journal can preserve program detection.
                has_programs: if changes.is_some_and(|c| c.draws.is_empty() && c.brushes.is_empty())
                {
                    metadata.has_programs
                } else {
                    has_program_brushes(canvas)
                },
            }
        } else {
            PreparedPlan::new(canvas, plan)
        };
        self.metadata = Some(PlanMetadata {
            fingerprint,
            stack_depths: prepared.stack_depths,
            has_programs: prepared.has_programs,
        });
        prepared
    }
}

/// Retained arena ranges and flat immediate frames share one text cache lifecycle.
/// The adapter owns its prepared data slot so localized resource swaps retain the
/// same cache ownership; selection and reconciliation are backend independent.
pub(crate) fn prepare_text(
    prepared: &mut Option<PreparedTextData>,
    canvas: &Canvas,
    font_system: &mut TextFontSystem,
    context: &mut TextContext,
) -> Option<PreparedTextChanges> {
    if let Some(text) = prepared {
        if let Some(changes) = &canvas.buffer_changes {
            text.update(
                &canvas.text_glyphs,
                &canvas.text_runs,
                &changes.glyphs,
                &changes.text_runs,
                font_system,
                context,
            );
            None
        } else {
            text.reconcile(&canvas.text_glyphs, &canvas.text_runs, font_system, context)
        }
    } else {
        *prepared = Some(PreparedTextData::new(
            &canvas.text_glyphs,
            &canvas.text_runs,
            font_system,
            context,
        ));
        None
    }
}

#[cfg(test)]
mod tests;
