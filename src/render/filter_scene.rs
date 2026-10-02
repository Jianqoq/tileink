//! Shared CPU preparation for isolated filter work. Full-canvas filters borrow
//! the original scene; translated filters compact only spatial candidates. This
//! preserves the existing allocation fast path independently of the GPU adapter.
use super::profile::cpu::profile_cpu;
use crate::{
    canvas::Canvas,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
        gpu_plan::{filter_scratch_extra, required_scratch_count},
        layer::filter::Filter,
        offscreen::{LocalOffscreenCanvas, local_filter, local_offscreen_scene},
    },
};

pub(crate) struct PreparedFilterScene<'a> {
    root: (&'a Canvas, &'a ExecPlan, &'a [ExecOp]),
    local: Option<LocalOffscreenCanvas>,
    filter: Filter,
}

impl<'a> PreparedFilterScene<'a> {
    pub(crate) fn new(
        canvas: &'a Canvas,
        plan: &'a ExecPlan,
        children: &'a [ExecOp],
        filter: &Filter,
        surface: Bounds,
        candidates: impl FnOnce() -> Vec<u32>,
    ) -> Self {
        let local = (surface != Bounds::canvas(canvas.physical_width(), canvas.physical_height()))
            .then(|| {
                let draws = candidates();
                profile_cpu("prepare.local_scene", || {
                    local_offscreen_scene(canvas, plan, children, surface, &draws)
                })
            });
        let filter = profile_cpu("prepare.local_filter", || local_filter(filter, surface));
        Self {
            root: (canvas, plan, children),
            local,
            filter,
        }
    }

    pub(crate) fn scene(&self) -> (&Canvas, &ExecPlan, &[ExecOp]) {
        self.local.as_ref().map_or(self.root, |local| {
            (&local.canvas, &local.plan, local.children.as_slice())
        })
    }

    pub(crate) fn filter(&self) -> &Filter {
        &self.filter
    }

    pub(crate) fn is_root_scene(&self) -> bool {
        self.local.is_none()
    }

    pub(crate) fn scratch_count(&self, cache_surface: bool) -> usize {
        // The cached path reserves filtered and unfiltered history in addition
        // to the source slot; child execution and filter temporaries share capacity.
        let reserved = if cache_surface { 3 } else { 1 };
        reserved + required_scratch_count(self.scene().1).max(filter_scratch_extra(&self.filter))
    }
}

#[cfg(test)]
mod tests;
