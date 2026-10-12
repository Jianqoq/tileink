//! Encode raster dispatches from the scene-owned bindings.
use super::super::{
    coarse::{self, CoarseBindings},
    fine::{self, FineBindings},
};
use super::*;
use crate::render::{
    coarse::{CoarseBatch, CoarsePlan},
    fine::FinePlan,
};
use std::ops::Range;

impl Scene {
    pub(crate) fn filter_candidates(
        &self,
        bounds: crate::shared::bounds::Bounds,
        canvas: &Canvas,
        plan: &ExecPlan,
    ) -> Vec<u32> {
        self.tile_draw_bins.draws_in_bounds(
            bounds,
            &plan.draw_order,
            canvas
                .retained_painter()
                .map(|painter| painter.keys.as_slice()),
        )
    }

    pub(crate) fn plan_handle(&self) -> Rc<ExecPlan> {
        std::rc::Rc::clone(&self.plan)
    }

    pub(crate) fn plan(&self) -> &ExecPlan {
        &self.plan
    }

    /// Select a logical batch ID, not a physical draw-record interval. Tile bins
    /// own physical indices; sparse retained IDs may exceed the draw count.
    pub(crate) fn encode_coarse(
        &self,
        batch: &mut ComputeBatch,
        batches: Range<u32>,
        layers: Range<u32>,
        chunked: bool,
        limit: u32,
    ) -> Result<FinePlan> {
        // Bound against the uploaded allocation, never replaceable plan metadata.
        if layers.end > self.layer_count {
            return Err("native coarse layer range exceeds prepared scene".into());
        }
        let selection = self.clip_dispatch.ranges.get(&(layers.start, layers.end));
        let active = selection
            .map(|r| r.end - r.start)
            .or(self.fine_params.active_tile_count);
        let mut plan = CoarsePlan::new(
            self.lengths,
            CoarseBatch {
                draw_start: batches.start,
                draw_end: batches.end,
                layer_stack_start: layers.start,
                layer_stack_end: layers.end,
                active_tile_count: active,
            },
            self.fine_params.paint_brush_base,
            chunked,
            limit,
        )?;
        if let Some(selection) = selection {
            plan.config.active_tile_list_base = selection.start;
        }
        if self.clip_dispatch.preallocated {
            plan.use_preallocated_tiles(self.lengths, limit)?;
        }
        let mut fine_plan = FinePlan::new(
            self.lengths,
            FineParams {
                active_tile_count: active,
                load_target: true,
                ..self.fine_params
            },
            limit,
        )?;
        if let Some(selection) = selection {
            fine_plan.set_active_tile_list_base(selection.start);
        }
        // SAFETY: this object owns handles allocated from the same validated
        // Canvas/plan association; layer indices are bounded, physical draw indices
        // come from its tile bins, and scan precedes coarse.
        unsafe {
            coarse::encode(
                batch,
                &plan,
                &CoarseBindings {
                    draws: self.draws,
                    text: self.coarse_text,
                    paint: self.paint,
                    paths: self.scan.paths,
                    backdrops: self.scan.backdrops,
                    ranges: self.scan.tile_segment_ranges,
                    layers: self.layers,
                    work: self.work,
                    chunks: self.chunks,
                    batches: self.batches,
                },
            )?;
        }
        Ok(fine_plan)
    }

    /// # Safety
    /// Image handles must match the placements supplied to `SceneCache::record`.
    /// Coarse for the current draw batch must precede fine in this `ComputeBatch`.
    pub(crate) unsafe fn encode_fine(
        &self,
        batch: &mut ComputeBatch,
        target: ResourceId,
        images: &SceneImages,
        plan: &FinePlan,
    ) -> Result<()> {
        // Canvas uploads and shared stack depths bound all record/spill
        // accesses; caller supplies associated images and stage ordering.
        let extended = self
            .program_images
            .as_ref()
            .map(|p| p.attach(batch, images))
            .transpose()?;
        let images = extended.as_ref().unwrap_or(images);
        // SAFETY: Canvas uploads and shared stack depths bound record/spill access; the caller guarantees associated images and coarse-before-fine ordering.
        unsafe {
            fine::encode(
                batch,
                plan,
                &FineBindings {
                    target,
                    draws: self.draws,
                    paint: self.paint,
                    coarse: self.work,
                    segments: self.scan.segments,
                    text: self.fine_text,
                    spills: self.spills,
                    atlas: images.atlas,
                    images: images.table,
                    sampler: images.sampler,
                },
            )?;
        }
        Ok(())
    }
}
