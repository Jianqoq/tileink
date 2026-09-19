use super::*;
use crate::{
    native::runtime::program::scene::PreparedScene,
    render::{
        damage_tiles::DamageTiles,
        draw_batches::execute_in_place_root_batches,
        frame::{FrameAdapter, FrameError},
    },
    shared::execution::ExecPlan,
    text::PreparedTextData,
};
use std::rc::Rc;

struct Frame<'gpu, 'scene> {
    prepared: Option<PreparedScene<'scene>>,
    batch: Option<&'gpu mut ComputeBatch>,
    images: &'gpu Images<'gpu>,
    text: Option<&'gpu PreparedTextData>,
    execution: Option<Execution<'gpu>>,
    size: (u32, u32),
    chunked: bool,
    limit: u32,
}

pub(super) fn record<'gpu>(
    cache: &mut SceneCache,
    batch: &'gpu mut ComputeBatch,
    canvas: &Canvas,
    images: &'gpu Images<'gpu>,
    text: Option<&'gpu PreparedTextData>,
    chunked: bool,
    limit: u32,
) -> Result<ResourceId> {
    // Validate before plan preparation or any native scene resources are recorded.
    images.validate(batch)?;
    let mut frame = Frame {
        prepared: Some(cache.prepare(canvas)),
        batch: Some(batch),
        images,
        text,
        execution: None,
        size: canvas.physical_size(),
        chunked,
        limit,
    };
    crate::render::frame::encode(&mut frame, canvas, false, false).map_err(
        |error| match error {
            FrameError::MissingPlan => {
                Box::<dyn std::error::Error>::from("native frame has no prepared plan")
            }
            FrameError::Adapter(error) => error,
        },
    )?;
    Ok(frame
        .execution
        .as_ref()
        .ok_or("native frame was not scanned")?
        .targets
        .get(RenderTargetId::Main)?
        .image())
}

impl FrameAdapter for Frame<'_, '_> {
    type Error = Box<dyn std::error::Error>;
    // Immediate frames own fresh targets. Submission retirement is the adapter's
    // responsibility and must not introduce a CPU wait into frame recording.
    fn recycle_previous_frame(&mut self) {}
    fn active_tiles(&self) -> Option<&DamageTiles> {
        None
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
        unreachable!("immediate native frames disable early submission")
    }
    fn scan_scene(&mut self, _canvas: &Canvas) -> Result<()> {
        self.execution = Some(Execution::prepare(
            self.prepared.take().ok_or("native frame already scanned")?,
            self.batch
                .take()
                .ok_or("native frame batch already consumed")?,
            self.images,
            self.text,
            self.chunked,
            self.limit,
        )?);
        Ok(())
    }
    fn clear_root(&mut self, partial: bool) -> Result<()> {
        if partial {
            return Err("native immediate frame cannot preserve partial history".into());
        }
        // Surface::allocate supplies zeroed pixels; this new root is transparent
        // already, without recording a redundant clear dispatch.
        Ok(())
    }
    fn active_batch_ids(&mut self, _batch_ids: &[u32]) -> Vec<u32> {
        unreachable!("immediate native frames have no damage selection")
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
        Err("native immediate frames do not own retained history".into())
    }
    fn record_filter_stats(&mut self) {}
}
