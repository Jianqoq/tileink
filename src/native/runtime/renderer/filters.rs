use super::*;
use crate::native::runtime::program::filter::stack::{Composite, Textures};
use crate::render::{
    damage_tiles::DamageTiles,
    filter_pass::FilterPassAdapter,
    filters::{FilterAdapter, FilterPlacement},
};
use crate::shared::{execution::ExecPlan, layer::filter::Filter};
use std::rc::Rc;

pub(crate) struct LocalState {
    scene: Option<Scene>,
    pending_plan: Option<Rc<ExecPlan>>,
    filters: filter_resources::FilterResources,
    targets: Targets,
    paths: Option<path_mask::Paths>,
    origin: (i32, i32),
    damage: Option<DamageTiles>,
}
impl FilterPassAdapter for Execution<'_> {
    fn copy_filter_region(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<()> {
        use crate::native::runtime::program::filter::{self, BasicFilter};
        let Some(c) = self.config(bounds) else {
            return Ok(());
        };
        filter::encode(
            self.batch,
            BasicFilter::Copy,
            c,
            None,
            Some(self.targets.get(source)?.image()),
            self.targets.get(target)?.image(),
        )
    }
    fn apply_filter_pass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        cursors: &mut FilterCursors,
    ) -> Result<()> {
        self.apply_filter(target, bounds, filter, None, cursors)
    }
    fn prepare_filter_output_work(&mut self) -> Result<()> {
        Ok(())
    }
}
impl FilterAdapter for Execution<'_> {
    type LocalState = LocalState;
    fn filter_candidates(&self, _bounds: Bounds, plan: &ExecPlan) -> Vec<u32> {
        // The shared localizer performs the exact bounds test and preserves draw order.
        plan.draw_order.as_ref().clone()
    }
    fn begin_filter_scene(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        _scratch: usize,
        origin: (i32, i32),
        _reuse_root: bool,
    ) -> Result<LocalState> {
        // Build the complete new context before replacing any parent state. Each
        // context owns disjoint resources, so nested recording cannot rewrite uniforms.
        let size = canvas.physical_size();
        let targets = Targets::new(self.batch, [size.0, size.1])?;
        let filters =
            filter_resources::FilterResources::record(self.batch, plan, Some(filter), self.images)?;
        let paths = prepare_paths(self.batch, plan)?;
        let pending_plan = Some(Rc::new(plan.clone()));
        let damage = self.retained.active_tiles().cloned();
        self.retained.set_active_tiles(None);
        Ok(LocalState {
            scene: self.scene.take(),
            pending_plan: std::mem::replace(&mut self.pending_plan, pending_plan),
            targets: std::mem::replace(&mut self.targets, targets),
            filters: std::mem::replace(&mut self.filters, filters),
            paths: std::mem::replace(&mut self.paths, paths),
            origin: std::mem::replace(&mut self.origin, origin),
            damage,
        })
    }
    fn end_filter_scene(&mut self, state: LocalState) {
        self.scene = state.scene;
        self.pending_plan = state.pending_plan;
        self.targets = state.targets;
        self.filters = state.filters;
        self.paths = state.paths;
        self.origin = state.origin;
        self.retained.set_active_tiles(state.damage);
    }
    fn prepare_filter_source_work(&mut self) -> Result<()> {
        Ok(())
    }
    fn scan_filter_scene(&mut self, canvas: &Canvas) -> Result<()> {
        let plan = self
            .pending_plan
            .take()
            .ok_or("native filter scene already scanned")?;
        // SAFETY: begin_filter_scene receives the matched Canvas/plan from the
        // shared PreparedFilterScene and neither is changed before scan.
        // Localized draws retain root glyph/run indices, so they must use the
        // same prepared text data rather than rebuilding a local glyph atlas.
        self.scene = Some(unsafe {
            SceneCache::default().record_with_plan(
                self.batch,
                canvas,
                self.text,
                Some(self.images.upload()),
                self.limit,
                plan,
            )?
        });
        Ok(())
    }
    fn composite_filter(
        &mut self,
        target: RenderTargetId,
        source: &Surface,
        placement: FilterPlacement,
        stack: Range<usize>,
    ) -> Result<()> {
        let Some(mut c) = self.config(placement.bounds) else {
            return Ok(());
        };
        c.offset_x = placement.origin.0;
        c.offset_y = placement.origin.1;
        c.kernel_columns = placement.size.0;
        c.kernel_rows = placement.size.1;
        c.layer_stack_start = u32::try_from(stack.start)?;
        c.layer_stack_end = u32::try_from(stack.end)?;
        self.scene
            .as_ref()
            .ok_or("native parent scene missing")?
            .filter_stack()
            .encode(
                self.batch,
                Composite::Surface,
                c,
                None,
                Textures {
                    source: source.image(),
                    auxiliary: None,
                    target: self.targets.get(target)?.image(),
                },
            )
    }
}
