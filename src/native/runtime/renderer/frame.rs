use super::FrameOptions;
use super::*;
use crate::{
    native::runtime::program::scene::PreparedScene,
    render::{
        damage_tiles::DamageTiles,
        draw_batches::execute_in_place_root_batches,
        frame::{FrameAdapter, FrameError},
    },
    shared::execution::ExecPlan,
};
use std::rc::Rc;

struct Frame<'gpu, 'scene> {
    prepared: Option<PreparedScene<'scene>>,
    batch: Option<&'gpu mut ComputeBatch>,
    resources: Option<super::FrameResources<'gpu>>,
    execution: Option<Execution<'gpu>>,
    size: (u32, u32),
    options: FrameOptions,
    limit: u32,
}

pub(super) fn record<'gpu>(
    cache: &mut SceneCache,
    batch: &'gpu mut ComputeBatch,
    canvas: &Canvas,
    resources: super::FrameResources<'gpu>,
    options: FrameOptions,
    limit: u32,
) -> Result<ResourceId> {
    // Validate before plan preparation or any native scene resources are recorded.
    resources.images.validate(batch)?;
    let mut frame = Frame {
        prepared: Some(cache.prepare(canvas)),
        batch: Some(batch),
        resources: Some(resources),
        execution: None,
        size: canvas.physical_size(),
        options,
        limit,
    };
    crate::render::frame::encode(
        &mut frame,
        canvas,
        false,
        crate::render::frame::SubmissionPolicy::Single,
    )
    .map_err(|error| match error {
        FrameError::MissingPlan => {
            Box::<dyn std::error::Error>::from("native frame has no prepared plan")
        }
        FrameError::Adapter(error) => error,
    })?;
    if let Some(execution) = &frame.execution {
        Ok(execution.targets.get(RenderTargetId::Main)?.image())
    } else {
        options
            .target
            .ok_or_else(|| "empty damage requires persistent native history".into())
    }
}

impl FrameAdapter for Frame<'_, '_> {
    type Error = Box<dyn std::error::Error>;
    // Submission retirement is the adapter's
    // responsibility and must not introduce a CPU wait into frame recording.
    fn recycle_previous_frame(&mut self) {}
    fn active_tiles(&self) -> Option<&DamageTiles> {
        if let Some(execution) = &self.execution {
            execution.retained.active_tiles()
        } else {
            self.resources
                .as_ref()
                .and_then(|resources| resources.retained.active_tiles())
        }
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn prepared_plan(&self) -> Option<Rc<ExecPlan>> {
        self.prepared.as_ref().map(PreparedScene::plan_handle)
    }
    fn portable_textures(&self) -> bool {
        false
    }
    fn prepare_frame_resources(&mut self) -> Result<()> {
        Ok(())
    }
    // Images is an already-resolved upload/texture pair. Vector child commands,
    // when present, precede the root in this same caller-owned ComputeBatch.
    fn encode_vector_images(&mut self) -> Result<()> {
        Ok(())
    }
    fn set_initial_root_batch_budget(&mut self, _budget: usize) {
        unreachable!("native frames disable early submission")
    }
    fn scan_scene(&mut self, _canvas: &Canvas) -> Result<()> {
        self.execution = Some(Execution::prepare(
            self.prepared.take().ok_or("native frame already scanned")?,
            self.batch
                .take()
                .ok_or("native frame batch already consumed")?,
            self.resources
                .take()
                .ok_or("native frame resources already consumed")?,
            self.options,
            self.limit,
        )?);
        Ok(())
    }
    fn clear_root(&mut self, partial: bool) -> Result<()> {
        if partial && self.options.target.is_none() {
            return Err("native partial frame requires persistent history".into());
        }
        if self.options.target.is_some() {
            let execution = self
                .execution
                .as_mut()
                .ok_or("native frame was not scanned")?;
            super::super::program::filter::encode(
                execution.batch,
                super::super::program::filter::BasicFilter::Clear,
                crate::shared::filter_config::FilterConfig {
                    width: self.size.0,
                    height: self.size.1,
                    region_width: self.size.0,
                    region_height: self.size.1,
                    clear_color: self.options.clear_color,
                    ..Default::default()
                },
                execution.retained.active_tiles().map(DamageTiles::list),
                None,
                execution.targets.get(RenderTargetId::Main)?.image(),
            )?;
        }
        // Fresh targets are initialized by upload; persistent targets are cleared
        // on the GPU at the shared root boundary before blending this frame.
        Ok(())
    }
    fn active_batch_ids(&mut self, _batch_ids: &[u32]) -> Vec<u32> {
        self.execution
            .as_ref()
            .expect("scanned native frame")
            .scene
            .as_ref()
            .expect("native scene")
            .active_batches
            .clone()
    }
    fn execute_direct_root(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> Result<()> {
        execute_in_place_root_batches(
            self.execution
                .as_mut()
                .ok_or("native frame was not scanned")?,
            canvas,
            &plan.ops,
            ops,
        )
    }
    fn execute_recursive(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        active: Option<&[u32]>,
    ) -> Result<()> {
        shared_operations::execute_ops(
            self.execution
                .as_mut()
                .ok_or("native frame was not scanned")?,
            canvas,
            plan,
            &plan.ops,
            RenderTargetId::Main,
            &mut FilterCursors::default(),
            active,
        )
    }
    fn copy_history_to_output(&mut self) -> Result<()> {
        Err("native target routing performs history copies after frame execution".into())
    }
    fn record_filter_stats(&mut self) {}
}
