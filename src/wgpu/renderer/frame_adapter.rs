use super::*;
use crate::render::frame::FrameAdapter;

/// WGPU resources for one call into the shared frame scheduler.
pub(super) struct WgpuFrameAdapter<'a> {
    renderer: &'a mut Renderer,
    commands: &'a mut WgpuCommandBatch,
    history_copy_dst: Option<&'a ::wgpu::Texture>,
}

impl<'a> WgpuFrameAdapter<'a> {
    pub(super) fn new(
        renderer: &'a mut Renderer,
        commands: &'a mut WgpuCommandBatch,
        history_copy_dst: Option<&'a ::wgpu::Texture>,
    ) -> Self {
        Self {
            renderer,
            commands,
            history_copy_dst,
        }
    }
}

impl FrameAdapter for WgpuFrameAdapter<'_> {
    type Error = ();

    fn recycle_previous_frame(&mut self) {
        // Prior frame batches have been submitted before queue writes can reuse
        // quarantined allocations. WGPU owns their physical in-flight lifetime.
        self.renderer.local_scene_resources.begin_frame();
    }
    fn active_tiles(&self) -> Option<&DamageTiles> {
        self.renderer.retained.active_tiles()
    }
    fn size(&self) -> (u32, u32) {
        self.renderer.size
    }
    fn prepared_plan(&self) -> Option<std::rc::Rc<ExecPlan>> {
        self.renderer.plan.clone()
    }
    fn portable_textures(&self) -> bool {
        self.renderer
            .fine
            .as_ref()
            .is_some_and(WgpuFinePipeline::uses_portable_textures)
    }
    fn prepare_frame_resources(&mut self) -> Result<(), ()> {
        if self.renderer.fine.is_none()
            || self.renderer.coarse_pipeline.is_none()
            || self.renderer.filter.is_none()
        {
            return Err(());
        }
        self.renderer
            .filter
            .as_ref()
            .unwrap()
            .reset_dispatch_counts();
        self.renderer.filter_tile_work_arena.reset();
        self.renderer.prepare_active_tile_buffers();
        Ok(())
    }
    fn encode_vector_images(&mut self) -> Result<(), ()> {
        self.renderer
            .encode_vector_images(self.commands)
            .then_some(())
            .ok_or(())
    }
    fn set_initial_root_batch_budget(&mut self, budget: usize) {
        self.commands.set_initial_root_batch_budget(budget);
    }
    fn scan_scene(&mut self, canvas: &Canvas) -> Result<(), ()> {
        self.renderer
            .scan_and_cumsum(self.commands, canvas)
            .then_some(())
            .ok_or(())
    }
    fn clear_root(&mut self, partial: bool) -> Result<(), ()> {
        if partial {
            self.renderer.clear_render_region(
                self.commands,
                RenderTargetId::Main,
                Bounds::canvas(self.renderer.size.0, self.renderer.size.1),
                self.renderer.clear_color,
            );
        } else {
            self.renderer.clear_render_target(
                self.commands,
                RenderTargetId::Main,
                self.renderer.clear_color,
            );
        }
        Ok(())
    }
    fn active_batch_ids(&mut self, ids: &[u32]) -> Vec<u32> {
        let tiles = self
            .renderer
            .retained
            .active_tiles()
            .expect("partial frame retains its active root work");
        self.renderer
            .scene_upload
            .active_batch_ids(tiles.list(), ids)
    }
    fn execute_direct_root(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> Result<(), ()> {
        self.renderer
            .execute_direct_root_batches(self.commands, canvas, plan, ops)
            .then_some(())
            .ok_or(())
    }
    fn execute_recursive(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        active: Option<&[u32]>,
    ) -> Result<(), ()> {
        self.renderer
            .execute_ops(
                self.commands,
                canvas,
                plan,
                &plan.ops,
                RenderTargetId::Main,
                &mut FilterCursors::default(),
                active,
            )
            .then_some(())
            .ok_or(())
    }
    fn copy_history_to_output(&mut self) -> Result<(), ()> {
        self.renderer
            .encode_history_copy(self.commands, self.history_copy_dst.ok_or(())?);
        Ok(())
    }
    fn record_filter_stats(&mut self) {
        if let Some(filter) = &self.renderer.filter {
            let (dispatches, compact_dispatches) = filter.dispatch_counts();
            let stats = self.renderer.retained.stats_mut();
            stats.filter_dispatches = dispatches;
            stats.compact_filter_dispatches = compact_dispatches;
        }
    }
}
