use super::program::filter::path_mask;
use super::{
    Result,
    compute::{ComputeBatch, ResourceId},
    program::scene::{Scene, SceneCache},
};
use crate::{
    Canvas,
    render::{
        draw_batches::DrawBatchAdapter,
        filter_resources::{cursors::FilterCursors, paths::FilterPathUpload},
        incremental::IncrementalRenderStats,
        operations as shared_operations,
        output::RenderTargetId,
        retained::RetainedRenderState,
    },
    shared::{bounds::Bounds, filter_config::FilterConfig},
};
use std::ops::Range;

mod images;
mod targets;
pub(crate) use images::Images;
mod groups;
mod masks;
mod operations;
mod surfaces;
use targets::{Surface, Targets};

/// Records one Canvas in an existing frame batch. Its image placements and scene
/// stay associated through recursive operations; errors invalidate the whole batch.
pub(crate) struct Execution<'a> {
    batch: &'a mut ComputeBatch,
    scene: Scene,
    images: &'a Images<'a>,
    targets: Targets,
    paths: Option<path_mask::Paths>,
    retained: RetainedRenderState<Surface>,
    chunked: bool,
    limit: u32,
}

impl<'a> Execution<'a> {
    pub(crate) fn record(
        cache: &mut SceneCache,
        batch: &'a mut ComputeBatch,
        canvas: &Canvas,
        images: &'a Images<'a>,
        chunked: bool,
        limit: u32,
    ) -> Result<ResourceId> {
        images.validate(batch)?;
        let size = canvas.physical_size();
        let targets = Targets::new(batch, [size.0, size.1])?;
        let scene = cache.record(batch, canvas, None, Some(images.upload()), limit)?;
        let paths = FilterPathUpload::from_plan(scene.plan());
        let paths = if paths.range_starts.is_empty() {
            None
        } else {
            let ranges: Vec<_> = paths
                .range_starts
                .iter()
                .zip(&paths.range_ends)
                .map(|(&start, &end)| start..end)
                .collect();
            let lines: Vec<_> = (0..paths.p0x.len())
                .map(|i| [paths.p0x[i], paths.p0y[i], paths.p1x[i], paths.p1y[i]])
                .collect();
            Some(path_mask::upload(batch, &ranges, &lines)?)
        };
        let plan = scene.plan().clone();
        let mut execution = Self {
            batch,
            scene,
            images,
            targets,
            paths,
            retained: RetainedRenderState::new(Default::default()),
            chunked,
            limit,
        };
        shared_operations::execute_ops(
            &mut execution,
            canvas,
            &plan,
            &plan.ops,
            RenderTargetId::Main,
            &mut FilterCursors::default(),
            None,
        )?;
        Ok(execution.targets.get(RenderTargetId::Main)?.image())
    }

    fn config(&self, bounds: Bounds) -> Option<FilterConfig> {
        let (width, height) = self.targets.size();
        let bounds = bounds.intersect(Bounds::canvas(width, height));
        (!bounds.is_empty()).then(|| FilterConfig {
            width,
            height,
            region_x0: bounds.x0 as u32,
            region_y0: bounds.y0 as u32,
            region_width: bounds.width(),
            region_height: bounds.height(),
            ..Default::default()
        })
    }
}

impl DrawBatchAdapter for Execution<'_> {
    type Error = Box<dyn std::error::Error>;
    fn stats_mut(&mut self) -> &mut IncrementalRenderStats {
        self.retained.stats_mut()
    }
    fn begin_root_batch(&mut self) -> Result<()> {
        Ok(())
    }
    fn coarse(&mut self, batches: Range<u32>, layers: Range<u32>) -> Result<()> {
        self.scene
            .encode_coarse(self.batch, batches, layers, self.chunked, self.limit)
    }
    fn fine(&mut self, target: RenderTargetId) -> Result<()> {
        let target = self.targets.get(target)?.image();
        // SAFETY: record owns the Scene/upload association, and the shared draw
        // scheduler records coarse immediately before this fine pass.
        unsafe {
            self.scene.encode_fine(
                self.batch,
                target,
                self.images.textures(),
                0,
                true,
                self.limit,
            )
        }
    }
}

#[cfg(test)]
#[path = "tests/frame_execution.rs"]
mod tests;
