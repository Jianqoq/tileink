//! Native GPU scene assembly from the same CPU plans used by wgpu.
use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use super::{
    coarse::{self, CoarseBindings},
    fine::{self, FineBindings},
    resources::{allocate, upload},
    scene_scan::{self, PreparedScan, ScanOutput},
};
use crate::{
    Canvas,
    render::{
        coarse::{CoarseBatch, CoarsePlan, validate_work_layout},
        fine::{FineParams, FinePlan, spill_layout},
        prepare::ScenePreparation,
        upload::{paint::PaintData, scene::SceneUploadStaging},
    },
    shared::{
        execution::ExecPlan,
        gpu_coarse::*,
        gpu_plan::{FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH, GpuBufferLengths},
        image_resource::GpuImageResourceUpload,
    },
    text::PreparedTextData,
};
use std::{ops::Range, rc::Rc};

#[derive(Default)]
pub(crate) struct SceneCache {
    staging: SceneUploadStaging,
    preparation: ScenePreparation,
    plan: Option<Rc<ExecPlan>>,
}

/// Keeps compiled metadata paired with its Canvas and cache until scan consumes it.
/// Dropping preparation leaves no uploaded plan falsely marked as reusable.
pub(crate) struct PreparedScene<'a> {
    cache: &'a mut SceneCache,
    canvas: &'a Canvas,
    plan: crate::render::prepare::PreparedPlan,
}

impl PreparedScene<'_> {
    pub(crate) fn plan_handle(&self) -> Rc<ExecPlan> {
        self.plan.plan.clone()
    }
    pub(crate) fn size(&self) -> (u32, u32) {
        self.canvas.physical_size()
    }
    pub(crate) fn record(
        self,
        batch: &mut ComputeBatch,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        limit: u32,
    ) -> Result<Scene> {
        self.cache
            .record_prepared(batch, self.canvas, text, images, limit, self.plan)
    }
}

pub(crate) struct Scene {
    draw_count: u32,
    plan: Rc<ExecPlan>,
    layer_count: u32,
    lengths: GpuBufferLengths,
    scan: ScanOutput,
    draws: ResourceId,
    paint: ResourceId,
    coarse_text: ResourceId,
    fine_text: ResourceId,
    layers: ResourceId,
    batches: ResourceId,
    work: ResourceId,
    chunks: ResourceId,
    spills: ResourceId,
    fine_params: FineParams,
}

/// These texture handles must come from the upload whose placements patched the
/// scene paint records. Their ownership/descriptor ABI is checked when binding.
pub(crate) struct SceneImages {
    pub(crate) atlas: ResourceId,
    pub(crate) table: ResourceId,
    pub(crate) sampler: ResourceId,
}

impl SceneCache {
    pub(crate) fn prepare<'a>(&'a mut self, canvas: &'a Canvas) -> PreparedScene<'a> {
        // Preparation updates the fingerprint even when it compiles a new plan.
        // Move the old plan out first: cancellation or recording failure must not
        // pair that old plan with the new fingerprint on the next attempt.
        let mut cached = self.plan.take();
        let plan = self.preparation.prepare_plan(canvas, &mut cached);
        PreparedScene {
            cache: self,
            canvas,
            plan,
        }
    }
    pub(crate) fn record(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        limit: u32,
    ) -> Result<Scene> {
        self.prepare(canvas).record(batch, text, images, limit)
    }

    // Localized plans carry remapped physical indices. Compiling the Canvas again
    // would sever that association and select unrelated draws or filter resources.
    /// # Safety
    /// The plan must be compiled for this Canvas or returned with it by the
    /// shared localizer; its physical draw/path/layer indices must remain paired.
    pub(crate) unsafe fn record_with_plan(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        limit: u32,
        plan: Rc<ExecPlan>,
    ) -> Result<Scene> {
        // Root cause: explicit local metadata has no Canvas fingerprint. Retaining
        // the previous fingerprint would reuse this plan for an unrelated root.
        self.preparation = ScenePreparation::default();
        self.plan = None;
        let prepared = crate::render::prepare::PreparedPlan {
            stack_depths: crate::shared::gpu_plan::plan_stack_depths(&plan),
            plan,
            reused_metadata: false,
            upload_filters: true,
        };
        self.record_prepared(batch, canvas, text, images, limit, prepared)
    }

    fn record_prepared(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        limit: u32,
        prepared: crate::render::prepare::PreparedPlan,
    ) -> Result<Scene> {
        let (scan, lengths) = PreparedScan::for_scene(canvas, &mut self.staging, text, &prepared);
        let work_words = validate_work_layout(lengths)?;
        let scan = scene_scan::encode_scene(batch, &scan, limit)?;
        let draws = upload(batch, &canvas.draw_records)?;
        let paint = self.staging.paint.prepare(canvas, images);
        let paint_offsets = (paint.shadow_base, paint.brush_base);
        let paint = match paint.data {
            PaintData::Immediate {
                sdfs,
                shadows,
                brushes,
            } => {
                let bytes = [
                    bytemuck::cast_slice::<_, u8>(sdfs),
                    bytemuck::cast_slice(shadows),
                    bytemuck::cast_slice(brushes),
                ]
                .concat();
                if bytes.is_empty() {
                    allocate(batch, 1, size_of::<u32>())?
                } else {
                    batch.buffer(bytes)?
                }
            }
            PaintData::Retained { words, .. } => upload(batch, words)?,
        };
        self.staging.text.refill(
            canvas,
            text,
            self.staging.text.atlas_signature,
            canvas.buffer_changes.as_ref(),
            None,
        );
        let coarse_text = upload(batch, &self.staging.text.coarse_blob)?;
        let fine_text = upload(batch, &self.staging.text.fine_blob)?;
        self.staging.layer_stack.clear();
        self.staging.layer_stack.extend(
            prepared
                .plan
                .layer_stack_data
                .iter()
                .copied()
                .map(LayerStackRecord::from),
        );
        let layers = upload(batch, &self.staging.layer_stack)?;
        let batches = upload(
            batch,
            canvas
                .stable_batch_ids
                .as_deref()
                .unwrap_or(&prepared.plan.draw_batch_ids),
        )?;
        let mut work = Vec::<u8>::new();
        work.try_reserve_exact(work_words.max(1) * size_of::<u32>())?;
        work.resize(work_words.max(1) * size_of::<u32>(), 0);
        let record_base = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let records: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_records());
        work[record_base * size_of::<u32>()..record_base * size_of::<u32>() + records.len()]
            .copy_from_slice(records);
        let index_base = coarse_work_tile_draw_index_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let indices: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_indices());
        work[index_base * size_of::<u32>()..index_base * size_of::<u32>() + indices.len()]
            .copy_from_slice(indices);
        let work = batch.buffer(work)?;
        let chunks = allocate(
            batch,
            lengths.coarse_chunk_count,
            size_of::<CoarseChunkRecord>(),
        )?;
        let clip_spill_depth = u32::try_from(
            prepared
                .stack_depths
                .0
                .saturating_sub(FINE_LOCAL_CLIP_DEPTH),
        )?;
        let group_spill_depth = u32::try_from(
            prepared
                .stack_depths
                .1
                .saturating_sub(FINE_LOCAL_GROUP_DEPTH),
        )?;
        let (_, spill_words) = spill_layout(
            u32::try_from(lengths.tile_count)?,
            clip_spill_depth,
            group_spill_depth,
        )?;
        let spills = allocate(batch, spill_words, size_of::<u32>())?;
        self.plan = Some(prepared.plan.clone());
        Ok(Scene {
            layer_count: u32::try_from(prepared.plan.layer_stack_data.len())?,
            draw_count: u32::try_from(canvas.draw_records.len())?,
            plan: prepared.plan,
            lengths,
            scan,
            draws,
            paint,
            coarse_text,
            fine_text,
            layers,
            batches,
            work,
            chunks,
            spills,
            fine_params: FineParams {
                width: canvas.physical_width(),
                height: canvas.physical_height(),
                clip_spill_depth,
                group_spill_depth,
                paint_sdf_shadow_base: paint_offsets.0,
                paint_brush_base: paint_offsets.1,
                text_image_base: self.staging.text.fine_image_base,
                text_image_data_base: self.staging.text.fine_image_data_base,
                ..Default::default()
            },
        })
    }
}

impl Scene {
    pub(crate) fn plan_handle(&self) -> Rc<ExecPlan> {
        self.plan.clone()
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
    ) -> Result<()> {
        // Bound against the uploaded allocation, never replaceable plan metadata.
        if layers.end > self.layer_count {
            return Err("native coarse layer range exceeds prepared scene".into());
        }
        let plan = CoarsePlan::new(
            self.lengths,
            CoarseBatch {
                draw_start: batches.start,
                draw_end: batches.end,
                layer_stack_start: layers.start,
                layer_stack_end: layers.end,
                active_tile_count: None,
            },
            self.fine_params.paint_brush_base,
            chunked,
            limit,
        )?;
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
            )
        }
    }

    /// # Safety
    /// Image handles must match the placements supplied to SceneCache::record.
    /// Coarse for the current draw batch must precede fine in this ComputeBatch.
    pub(crate) unsafe fn encode_fine(
        &self,
        batch: &mut ComputeBatch,
        target: ResourceId,
        images: &SceneImages,
        clear_color: u32,
        load_target: bool,
        limit: u32,
    ) -> Result<()> {
        let plan = FinePlan::new(
            self.lengths,
            FineParams {
                clear_color,
                load_target,
                ..self.fine_params
            },
            limit,
        )?;
        // SAFETY: Canvas uploads and shared stack depths bound all record/spill
        // accesses; caller supplies associated images and stage ordering.
        unsafe {
            fine::encode(
                batch,
                &plan,
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
            )
        }
    }
}

#[cfg(test)]
#[path = "../tests/scene.rs"]
mod tests;

#[path = "scene/images.rs"]
mod images;

#[path = "scene/layers.rs"]
mod layers;

#[path = "scene/vector_images.rs"]
mod vector_images;
