//! Per-renderer CPU preparation shared by every GPU adapter.
//!
//! Keep the outer scene's metadata separate from the adapter's temporary local
//! execution plan. Patched descriptor values must consume Canvas's current plan
//! while retaining topology metadata; reusing the old plan would execute stale bounds.

use std::rc::Rc;

use crate::shared::execution::{ExecPlan, ROOT_COMMAND_LIST_ID};
use crate::shared::gpu_plan::plan_stack_depths;
#[cfg(test)]
use crate::shared::gpu_plan::required_scratch_count;
use crate::text::{PreparedTextChanges, PreparedTextData};
use crate::{Canvas, TextContext, TextFontSystem};

use super::profile::cpu::profile_cpu;

#[derive(Default)]
pub(crate) struct ScenePreparation {
    fingerprint: Option<u64>,
    stack_depths: (usize, usize),
}

pub(crate) struct PreparedPlan {
    pub(crate) plan: Rc<ExecPlan>,
    pub(crate) reused_metadata: bool,
    pub(crate) stack_depths: (usize, usize),
    #[cfg(test)]
    pub(crate) upload_filters: bool,
}

impl PreparedPlan {
    #[cfg(test)]
    /// Keep scratch planning and adapter allocation in the existing measured scope.
    /// This preserves stage attribution when preparation moves out of an adapter.
    pub(crate) fn prepare_scratch(&self, allocate: impl FnOnce(usize)) {
        if !self.reused_metadata {
            profile_cpu("prepare.scratch", || {
                allocate(required_scratch_count(&self.plan))
            });
        }
    }
}

impl ScenePreparation {
    pub(crate) fn prepare_plan(
        &mut self,
        canvas: &Canvas,
        cached_plan: &mut Option<Rc<ExecPlan>>,
    ) -> PreparedPlan {
        let fingerprint = canvas.execution_plan_fingerprint();
        let changes = canvas.buffer_changes.as_ref();
        // Stable structure preserves allocation/depth metadata, not parameter values.
        // Native retained updates can replace the materializer's plan while the
        // renderer still owns the previous Rc (for example an opacity-only edit).
        let exact_reuse = cached_plan.is_some() && self.fingerprint == Some(fingerprint);
        let reused_metadata = cached_plan.is_some()
            && (exact_reuse
                || changes.is_some_and(|changes| {
                    changes.plan_structure_reused || changes.plan_values_patched
                }));
        let plan = profile_cpu("prepare.compile", || {
            if exact_reuse {
                cached_plan.take().expect("cached execution plan")
            } else {
                canvas.compile_shared(ROOT_COMMAND_LIST_ID)
            }
        });
        if !reused_metadata {
            self.stack_depths = profile_cpu("prepare.stack_depths", || plan_stack_depths(&plan));
        }
        self.fingerprint = Some(fingerprint);
        PreparedPlan {
            plan,
            reused_metadata,
            stack_depths: self.stack_depths,
            #[cfg(test)]
            upload_filters: !reused_metadata
                || changes.is_some_and(|changes| changes.filter_resources_changed),
        }
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
    profile_cpu("prepare.text", || {
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
    })
}

#[cfg(test)]
mod tests;
