//! WGPU resources and encoding for the shared draw-batch algorithm.
use super::*;
use crate::render::draw_batches::{DrawBatchAdapter, PingPongSide, RootBatchAdapter};
use crate::render::incremental::IncrementalRenderStats;
use std::ops::Range;

mod groups;

pub(super) struct WgpuExecutionAdapter<'a> {
    renderer: &'a mut Renderer,
    commands: &'a mut WgpuCommandBatch,
    portable_root: Option<::wgpu::Texture>,
}

impl<'a> WgpuExecutionAdapter<'a> {
    pub(super) fn new(renderer: &'a mut Renderer, commands: &'a mut WgpuCommandBatch) -> Self {
        Self {
            renderer,
            commands,
            portable_root: None,
        }
    }

    fn target(renderer: &Renderer, side: PingPongSide) -> &WgpuTarget {
        match side {
            PingPongSide::Source => &renderer.fine_portable_source,
            PingPongSide::Destination => &renderer.fine_portable_target,
        }
    }
}

impl DrawBatchAdapter for WgpuExecutionAdapter<'_> {
    type Error = ();

    fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        self.renderer.retained.stats_mut()
    }

    fn begin_root_batch(&mut self) -> Result<(), ()> {
        self.commands.begin_root_batch();
        Ok(())
    }

    fn coarse(&mut self, batches: Range<u32>, layers: Range<u32>) -> Result<(), ()> {
        self.renderer
            .encode_coarse_batch(
                self.commands,
                batches.start,
                batches.end,
                layers.start,
                layers.end,
            )
            .then_some(())
            .ok_or(())
    }

    fn fine(&mut self, target: RenderTargetId) -> Result<(), ()> {
        self.renderer
            .fine_batch_to_in(self.commands, target)
            .then_some(())
            .ok_or(())
    }
}

impl RootBatchAdapter for WgpuExecutionAdapter<'_> {
    fn prepare_portable_targets(&mut self) -> Result<(), ()> {
        self.portable_root = Some(
            self.renderer
                .render_target_texture(RenderTargetId::Main)
                .cloned()
                .ok_or(())?,
        );
        self.renderer.fine_portable_source.resize(
            self.commands.device(),
            self.renderer.size.0,
            self.renderer.size.1,
        );
        self.renderer.fine_portable_target.resize(
            self.commands.device(),
            self.renderer.size.0,
            self.renderer.size.1,
        );
        Ok(())
    }

    fn copy_root_to(&mut self, side: PingPongSide) -> Result<(), ()> {
        copy_texture(
            self.commands.encoder(),
            self.portable_root.as_ref().ok_or(())?,
            Self::target(self.renderer, side).texture(),
            self.renderer.size,
        );
        Ok(())
    }

    fn copy_to_root(&mut self, side: PingPongSide) -> Result<(), ()> {
        copy_texture(
            self.commands.encoder(),
            Self::target(self.renderer, side).texture(),
            self.portable_root.as_ref().ok_or(())?,
            self.renderer.size,
        );
        Ok(())
    }

    fn fine_portable(&mut self, source: PingPongSide) -> Result<(), ()> {
        self.renderer
            .fine_portable_batch_to_views_in(
                self.commands,
                Self::target(self.renderer, source).view(),
                Self::target(self.renderer, source.other()).view(),
            )
            .then_some(())
            .ok_or(())
    }
}

impl crate::render::operations::OperationAdapter for WgpuExecutionAdapter<'_> {
    fn offscreen(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: crate::render::operations::Offscreen<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), ()> {
        crate::render::layers::execute(self, canvas, plan, op, target, cursors).map_err(|_| ())
    }

    fn mask(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        op: crate::render::operations::Masked<'_>,
        target: RenderTargetId,
        cursors: &mut FilterCursors,
    ) -> Result<(), ()> {
        crate::render::masks::execute(self, canvas, plan, op, target, cursors)
    }
}

mod masks;
mod surfaces;

mod filters;

mod backdrops;
