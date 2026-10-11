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
    shared::{bounds::Bounds, execution::ExecPlan, filter_config::FilterConfig},
};
use std::{ops::Range, rc::Rc};

mod backdrops;
mod filter_encoding;
mod filter_kernels;
mod filter_resources;
mod filter_scenes;
mod filters;
mod frame;
pub(crate) mod images;
pub(crate) mod recording;
mod targets;
pub(crate) use images::Images;
mod groups;
mod masks;
mod operations;
mod surfaces;
use targets::{Surface, Targets};

/// Root initialization and coarse scheduling choices for one immediate frame.
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameOptions {
    pub chunked: bool,
    pub clear_color: u32,
    pub target: Option<ResourceId>,
}

struct FrameResources<'a> {
    filter_scenes: filter_scenes::FilterScenes<'a>,
    images: &'a Images<'a>,
    text: Option<&'a crate::text::PreparedTextData>,
    retained: &'a mut RetainedRenderState<Surface>,
}

/// Filter damage is chosen after entering its context and before scan. Store
/// exactly one stage, so a context can never have both (or neither) plan and scene.
#[allow(clippy::large_enum_variant)] // Keep the scanned scene inline; avoid per-context heap allocations.
enum SceneState {
    Pending(Rc<ExecPlan>),
    Scanned(Scene),
}

impl SceneState {
    fn scanned(&self) -> &Scene {
        match self {
            Self::Scanned(scene) => scene,
            Self::Pending(_) => unreachable!("scene is scanned before drawing or compositing"),
        }
    }
}

/// Records one Canvas in an existing frame batch. Its image placements and scene
/// stay associated through recursive operations; errors invalidate the whole batch.
pub(crate) struct Execution<'a> {
    filter_scenes: filter_scenes::FilterScenes<'a>,
    batch: &'a mut ComputeBatch,
    scene: SceneState,
    filters: filter_resources::FilterResources,
    origin: (i32, i32),
    images: &'a Images<'a>,
    text: Option<&'a crate::text::PreparedTextData>,
    targets: Targets,
    paths: Option<path_mask::Paths>,
    retained: &'a mut RetainedRenderState<Surface>,
    chunked: bool,
    limit: u32,
}

impl<'a> Execution<'a> {
    pub(crate) fn record(
        cache: &mut SceneCache,
        batch: &'a mut ComputeBatch,
        canvas: &Canvas,
        images: &'a Images<'a>,
        text: Option<&'a crate::text::PreparedTextData>,
        options: FrameOptions,
        limit: u32,
    ) -> Result<ResourceId> {
        frame::record(
            cache,
            batch,
            canvas,
            FrameResources {
                filter_scenes: filter_scenes::FilterSceneCache::default().frame(),
                images,
                text,
                retained: &mut RetainedRenderState::default(),
            },
            options,
            limit,
        )
    }

    fn prepare(
        prepared: crate::native::runtime::program::scene::PreparedScene<'_>,
        batch: &'a mut ComputeBatch,
        resources: FrameResources<'a>,
        options: FrameOptions,
        limit: u32,
    ) -> Result<Self> {
        let FrameResources {
            filter_scenes,
            images,
            text,
            retained,
        } = resources;
        let size = prepared.size();
        let targets = if let Some(target) = options.target {
            Targets::from_image(batch, target, [size.0, size.1])?
        } else {
            Targets::new(batch, [size.0, size.1], options.clear_color)?
        };
        let scene = prepared.record(
            batch,
            text,
            Some(images.upload()),
            super::program::scene::SceneOptions {
                limit,
                active: retained.active_tiles(),
            },
        )?;
        // Only offscreen operations can own filter tables or mask paths. The
        // direct-root index is absent whenever an offscreen operation exists.
        let (filters, paths) = if needs_offscreen_resources(scene.plan()) {
            (
                filter_resources::FilterResources::record(batch, scene.plan(), None, images)?,
                prepare_paths(batch, scene.plan())?,
            )
        } else {
            (filter_resources::FilterResources::default(), None)
        };
        Ok(Self {
            filter_scenes,
            batch,
            scene: SceneState::Scanned(scene),
            filters,
            origin: (0, 0),
            images,
            text,
            targets,
            paths,
            retained,
            chunked: options.chunked,
            limit,
        })
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
    fn draw(
        &mut self,
        batches: Range<u32>,
        layers: Range<u32>,
        target: RenderTargetId,
    ) -> Result<()> {
        let scene = self.scene.scanned();
        let fine = scene.encode_coarse(self.batch, batches, layers, self.chunked, self.limit)?;
        let target = self.targets.get(target)?.image();
        // SAFETY: this context owns the Scene/upload association, and the local
        // fine plan comes from the coarse pass recorded immediately above.
        unsafe { scene.encode_fine(self.batch, target, self.images.textures(), &fine) }
    }
}

#[cfg(test)]
#[path = "tests/frame_execution.rs"]
mod tests;

fn needs_offscreen_resources(plan: &crate::shared::execution::ExecPlan) -> bool {
    plan.direct_root_batch_ops.is_none()
}

fn prepare_paths(
    batch: &mut ComputeBatch,
    plan: &crate::shared::execution::ExecPlan,
) -> Result<Option<path_mask::Paths>> {
    let paths = FilterPathUpload::from_plan(plan);
    if paths.range_starts.is_empty() {
        return Ok(None);
    }
    let ranges: Vec<_> = paths
        .range_starts
        .iter()
        .zip(&paths.range_ends)
        .map(|(&start, &end)| start..end)
        .collect();
    let lines: Vec<_> = (0..paths.p0x.len())
        .map(|i| [paths.p0x[i], paths.p0y[i], paths.p1x[i], paths.p1y[i]])
        .collect();
    Ok(Some(path_mask::upload(batch, &ranges, &lines)?))
}
