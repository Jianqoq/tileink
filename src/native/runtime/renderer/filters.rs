use super::*;
use crate::native::runtime::program::filter::stack::{Composite, Textures};
use crate::render::{
    damage_tiles::DamageTiles,
    filter_pass::FilterPassAdapter,
    filters::{FilterAdapter, FilterPlacement},
};
use crate::shared::{execution::ExecPlan, layer::filter::Filter};

pub(crate) struct LocalState {
    scene: SceneState,
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
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
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
}
impl FilterAdapter for Execution<'_> {
    type LocalState = LocalState;
    fn filter_candidates(&self, bounds: Bounds, canvas: &Canvas, plan: &ExecPlan) -> Vec<u32> {
        self.scene.scanned().filter_candidates(bounds, canvas, plan)
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
        let targets = Targets::new(self.batch, [size.0, size.1], 0)?;
        let filters =
            filter_resources::FilterResources::record(self.batch, plan, Some(filter), self.images)?;
        let paths = prepare_paths(self.batch, plan)?;
        let scene = SceneState::Pending(Rc::new(plan.clone()));
        let damage = self.retained.active_tiles().cloned();
        self.retained.set_active_tiles(None);
        Ok(LocalState {
            scene: std::mem::replace(&mut self.scene, scene),
            targets: std::mem::replace(&mut self.targets, targets),
            filters: std::mem::replace(&mut self.filters, filters),
            paths: std::mem::replace(&mut self.paths, paths),
            origin: std::mem::replace(&mut self.origin, origin),
            damage,
        })
    }
    fn end_filter_scene(&mut self, state: LocalState) {
        self.scene = state.scene;
        self.targets = state.targets;
        self.filters = state.filters;
        self.paths = state.paths;
        self.origin = state.origin;
        self.retained.set_active_tiles(state.damage);
    }
    fn scan_filter_scene(&mut self, canvas: &Canvas) -> Result<()> {
        let SceneState::Pending(plan) = &self.scene else {
            unreachable!("filter scan follows context preparation");
        };
        // SAFETY: begin_filter_scene receives the matched Canvas/plan from the
        // shared PreparedFilterScene and neither is changed before scan.
        // Localized draws retain root glyph/run indices, so they must use the
        // same prepared text data rather than rebuilding a local glyph atlas.
        let scene = unsafe {
            self.filter_scenes.next().record_with_plan(
                self.batch,
                canvas,
                self.text,
                Some(self.images.upload()),
                super::super::program::scene::SceneOptions {
                    limit: self.limit,
                    active: self.retained.active_tiles(),
                },
                Rc::clone(plan),
            )?
        };
        self.scene = SceneState::Scanned(scene);
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
        let source = source.image_in(self.batch)?;
        self.scene.scanned().filter_clip_stack().encode(
            self.batch,
            Composite::Surface,
            c,
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
            Textures {
                source,
                auxiliary: None,
                target: self.targets.get(target)?.image(),
            },
        )
    }
}
