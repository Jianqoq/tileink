use super::*;
use crate::render::filter_pass::FilterPassAdapter;
use crate::render::filters::{FilterAdapter, FilterPlacement};
use std::ops::Range;

// Allocation ownership remains in WGPU. The shared executor restores this
// state on every recording exit, before compositing or publishing a cache.
pub(in crate::wgpu::renderer) struct FilterSceneState {
    local: Option<SavedRendererState>,
    damage: Option<DamageTiles>,
    work: Option<FilterTileWork>,
}

impl FilterPassAdapter for WgpuExecutionAdapter<'_> {
    fn copy_filter_region(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), ()> {
        self.renderer
            .copy_region_to_target(self.commands, source, target, bounds)
            .then_some(())
            .ok_or(())
    }
    fn apply_filter_pass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        cursors: &mut FilterCursors,
    ) -> Result<(), ()> {
        self.renderer
            .apply_filter(self.commands, target, bounds, filter, None, cursors)
            .then_some(())
            .ok_or(())
    }
    fn prepare_filter_output_work(&mut self) -> Result<(), ()> {
        self.renderer.prepare_filter_active_tile_work();
        Ok(())
    }
}

impl FilterAdapter for WgpuExecutionAdapter<'_> {
    type LocalState = FilterSceneState;
    fn filter_candidates(&self, bounds: Bounds, plan: &ExecPlan) -> Vec<u32> {
        self.renderer.scene_upload.draws_in_bounds(bounds, plan)
    }
    fn begin_filter_scene(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        scratch: usize,
        origin: (i32, i32),
        reuse_root: bool,
    ) -> Result<FilterSceneState, ()> {
        let renderer = &mut self.renderer;
        // Fix queue-write aliasing at the resource boundary. Halo scan/coarse
        // worklists need separate storage: rewriting root buffers would change
        // earlier and later draws in the same submission, even after CPU state
        // is restored. Pointwise scopes keep the existing reuse fast path.
        let changes_active_work = renderer.retained.active_tiles().is_some()
            && crate::shared::layer::filter::filter_dependency(filter)
                != crate::shared::layer::filter::FilterDependency::Local(0);
        // Resetting an occupied pool would also discard the caller's live slots.
        if reuse_root && !changes_active_work && !renderer.scratch_slots.any_occupied() {
            let state = FilterSceneState {
                local: None,
                damage: renderer.retained.active_tiles().cloned(),
                work: renderer
                    .filter
                    .as_ref()
                    .and_then(WgpuFilterPipeline::active_tile_work),
            };
            renderer.prepare_scratch_buffers(scratch.max(1));
            Ok(state)
        } else {
            Ok(FilterSceneState {
                local: Some(
                    renderer.activate_local_scene_resources(canvas, plan, filter, scratch, origin),
                ),
                damage: None,
                work: None,
            })
        }
    }
    fn end_filter_scene(&mut self, state: FilterSceneState) {
        let renderer = &mut self.renderer;
        renderer.scratch_slots.release_all();
        if let Some(local) = state.local {
            renderer.restore_root_scene_resources(local);
        } else {
            renderer.retained.set_active_tiles(state.damage);
            if let Some(filter) = renderer.filter.as_mut() {
                filter.restore_active_tile_work(state.work);
            }
        }
    }
    fn prepare_filter_source_work(&mut self) -> Result<(), ()> {
        self.renderer.prepare_active_tile_buffers();
        Ok(())
    }
    fn scan_filter_scene(&mut self, canvas: &Canvas) -> Result<(), ()> {
        self.renderer
            .scan_and_cumsum(self.commands, canvas)
            .then_some(())
            .ok_or(())
    }
    fn composite_filter(
        &mut self,
        target: RenderTargetId,
        source: &WgpuTarget,
        placement: FilterPlacement,
        stack: Range<usize>,
    ) -> Result<(), ()> {
        self.renderer
            .composite_cached_filter_surface(
                self.commands,
                target,
                source,
                placement.size,
                placement.origin,
                placement.bounds,
                stack,
            )
            .then_some(())
            .ok_or(())
    }
}
