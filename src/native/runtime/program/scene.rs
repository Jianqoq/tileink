//! Native GPU scene assembly from the same CPU plans used by wgpu.
use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use super::{
    coarse::{self, CoarseBindings},
    fine::{self, FineBindings},
    resources::allocate,
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
        gpu_plan::{
            FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH, GpuBufferLengths, TILE_DRAW_PAGE_WORDS,
        },
        image_resource::GpuImageResourceUpload,
    },
    text::PreparedTextData,
};
use std::{ops::Range, rc::Rc};

#[derive(Default)]
struct SceneBuffers {
    work: super::cached_buffer::CachedBuffer,
    tile_bin_layout: Option<(usize, usize, usize, usize)>,
    chunks: super::cached_buffer::CachedBuffer,
    spills: super::cached_buffer::CachedBuffer,
    coarse_text: super::cached_buffer::CachedBuffer,
    fine_text: super::cached_buffer::CachedBuffer,
    scan: scene_scan::ScanBuffers,
    draws: super::cached_buffer::CachedBuffer,
    paint: super::cached_buffer::CachedBuffer,
    layers: super::cached_buffer::CachedBuffer,
    batches: super::cached_buffer::CachedBuffer,
}

#[derive(Default)]
pub(crate) struct SceneCache {
    buffers: SceneBuffers,
    pending: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    staging: SceneUploadStaging,
    preparation: ScenePreparation,
    plan: Option<Rc<ExecPlan>>,
}

pub(crate) struct SceneOptions<'a> {
    pub limit: u32,
    pub active: Option<&'a crate::render::damage_tiles::DamageTiles>,
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
        options: SceneOptions<'_>,
    ) -> Result<Scene> {
        self.cache
            .record_prepared(batch, self.canvas, text, images, options, self.plan)
    }
}

pub(crate) struct Scene {
    pub(crate) active_batches: Vec<u32>,
    // Share the prepared spatial index without copying it each frame. A later
    // preparation uses copy-on-write if an older recorded scene is still alive.
    tile_draw_bins: Rc<crate::shared::gpu_plan::TileDrawBins>,
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
    clip_dispatch: clip_tiles::ClipDispatch,
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
        self.prepare(canvas).record(
            batch,
            text,
            images,
            SceneOptions {
                limit,
                active: None,
            },
        )
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
        options: SceneOptions<'_>,
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
            #[cfg(test)]
            upload_filters: true,
        };
        self.record_prepared(batch, canvas, text, images, options, prepared)
    }

    fn record_prepared(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        images: Option<&GpuImageResourceUpload>,
        options: SceneOptions<'_>,
        prepared: crate::render::prepare::PreparedPlan,
    ) -> Result<Scene> {
        if self
            .pending
            .as_ref()
            .is_some_and(|accepted| !accepted.get())
        {
            // A failed frame may have consumed shared dirty journals before reaching
            // every upload. Rebuild all GPU scene storage rather than apply a gap.
            self.buffers = SceneBuffers::default();
        }
        self.pending = Some(batch.acceptance());
        let limit = options.limit;
        let (scan, lengths) = PreparedScan::for_scene(canvas, &mut self.staging, text, &prepared);
        let work_words = validate_work_layout(lengths)?;
        let output = scene_scan::encode_cached(batch, &scan, limit, &mut self.buffers.scan)?;
        let dirty = scan.dirty;
        self.staging.path_plans.recycle_dirty(dirty);
        let scan = output;
        let draws = self.buffers.draws.upload(
            batch,
            &canvas.draw_records,
            canvas
                .buffer_changes
                .as_ref()
                .map(|changes| changes.draws.as_slice()),
        )?;
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
            PaintData::Retained { words, ranges } => {
                self.buffers.paint.upload(batch, words, Some(&ranges))?
            }
        };
        self.staging.text.refill(
            canvas,
            text,
            self.staging.text.atlas_signature,
            canvas.buffer_changes.as_ref(),
            None,
        );
        let coarse_text = self.buffers.coarse_text.upload(
            batch,
            &self.staging.text.coarse_blob,
            Some(&self.staging.text.dirty_coarse),
        )?;
        let fine_text = self.buffers.fine_text.upload(
            batch,
            &self.staging.text.fine_blob,
            Some(&self.staging.text.dirty_fine),
        )?;
        self.staging.layer_stack.clear();
        self.staging.layer_stack.extend(
            prepared
                .plan
                .layer_stack_data
                .iter()
                .copied()
                .map(LayerStackRecord::from),
        );
        let layer_ranges = canvas
            .buffer_changes
            .as_ref()
            .filter(|_| prepared.reused_metadata)
            .map(|changes| changes.plan_layer_stack.as_slice());
        let layers = self
            .buffers
            .layers
            .upload(batch, &self.staging.layer_stack, layer_ranges)?;
        let batch_ranges = canvas.buffer_changes.as_ref().map(|changes| {
            crate::render::upload::ranges::merge_sorted_dirty_ranges(
                &changes.draws,
                &changes.painter,
            )
        });
        let batches = self.buffers.batches.upload(
            batch,
            canvas
                .stable_batch_ids
                .as_deref()
                .unwrap_or(&prepared.plan.draw_batch_ids),
            if canvas.stable_batch_ids.is_some() {
                batch_ranges.as_deref()
            } else {
                None
            },
        )?;
        let active_batches = if let Some(active) = options.active {
            if active.dimensions() != (lengths.tiles_width as u32, lengths.tiles_height as u32) {
                return Err("native damage dimensions differ from scene".into());
            }
            self.staging.active_batch_ids(
                active.list(),
                canvas
                    .stable_batch_ids
                    .as_deref()
                    .unwrap_or(&prepared.plan.draw_batch_ids),
            )
        } else {
            Vec::new()
        };
        // Scan/coarse dispatches initialize their GPU outputs. Upload only the CPU
        // tile bins and active list, rather than clearing and transferring the whole
        // work arena (including PTCL, glyph and emit scratch) on every frame.
        let record_base = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        ) * size_of::<u32>();
        let index_base = coarse_work_tile_draw_index_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        ) * size_of::<u32>();
        let mut clip_dispatch = clip_tiles::ClipDispatch::new(
            canvas,
            &prepared.plan,
            options.active.map(|a| a.list()),
            self.staging.tile_draw_bins.upload_records(),
            lengths,
            prepared.stack_depths.0,
        )?;
        let (bins_full, dirty_records, dirty_pages) =
            Rc::make_mut(&mut self.staging.tile_draw_bins).take_dirty();
        let mut prefix_updates = Vec::new();
        if !clip_dispatch.slots.is_empty() {
            prefix_updates.push((0, bytemuck::cast_slice::<_, u8>(&clip_dispatch.slots)));
        }
        let records: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_records());
        let indices: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_indices());
        let mut tail_updates = Vec::new();
        if !clip_dispatch.kinds.is_empty() {
            let offset = crate::shared::gpu_coarse::coarse_work_fine_tile_kind_word_offset(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
                lengths.tile_draw_index_count,
                lengths.tile_draw_chunk_count,
            ) * size_of::<u32>();
            tail_updates.push((
                offset,
                bytemuck::cast_slice::<u32, u8>(&clip_dispatch.kinds),
            ));
        }
        if let Some(active) = options.active {
            let offset = crate::shared::gpu_coarse::coarse_work_active_tile_list_word_offset(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
                lengths.tile_draw_index_count,
                lengths.tile_draw_chunk_count,
            ) * size_of::<u32>();
            let bytes: &[u8] = bytemuck::cast_slice(active.list());
            if !bytes.is_empty() {
                tail_updates.push((offset, bytes));
            }
        }
        if !clip_dispatch.data.is_empty() {
            tail_updates.push((
                work_words * size_of::<u32>(),
                bytemuck::cast_slice::<u32, u8>(&clip_dispatch.data),
            ));
        }
        let mut full_bin_updates = Vec::new();
        if !records.is_empty() {
            full_bin_updates.push((record_base, records));
        }
        if !indices.is_empty() {
            full_bin_updates.push((index_base, indices));
        }
        let mut full_updates = prefix_updates.clone();
        full_updates.extend_from_slice(&full_bin_updates);
        full_updates.extend_from_slice(&tail_updates);
        let layout = (record_base, index_base, records.len(), indices.len());
        let mut dirty_updates = prefix_updates;
        if bins_full || self.buffers.tile_bin_layout != Some(layout) {
            dirty_updates.extend_from_slice(&full_bin_updates);
        } else {
            dirty_updates.extend(tile_bin_dirty_updates(
                self.staging.tile_draw_bins.upload_records(),
                self.staging.tile_draw_bins.upload_indices(),
                record_base,
                index_base,
                &dirty_records,
                &dirty_pages,
            ));
        }
        dirty_updates.extend_from_slice(&tail_updates);
        let work = self.buffers.work.patches_delta(
            batch,
            (work_words + clip_dispatch.data.len()).max(1) * size_of::<u32>(),
            &full_updates,
            &dirty_updates,
        )?;
        self.buffers.tile_bin_layout = Some(layout);
        clip_dispatch.discard_upload_data();
        Rc::make_mut(&mut self.staging.tile_draw_bins).recycle_dirty(dirty_records, dirty_pages);
        let chunks = self.buffers.chunks.scratch(
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
        let spills = self
            .buffers
            .spills
            .scratch(batch, spill_words, size_of::<u32>())?;
        self.plan = Some(prepared.plan.clone());
        Ok(Scene {
            active_batches,
            clip_dispatch,
            layer_count: u32::try_from(prepared.plan.layer_stack_data.len())?,
            tile_draw_bins: Rc::clone(&self.staging.tile_draw_bins),
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
                active_tile_count: options.active.map(|active| active.len()),
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

/// Convert sorted dirty tile IDs into bounded, coalesced regions of the page arena.
/// The complete snapshot remains available when storage/layout changes or a batch fails.
fn tile_bin_dirty_updates<'a>(
    records: &'a [TileDrawRecord],
    indices: &'a [u32],
    record_base: usize,
    index_base: usize,
    dirty_records: &[usize],
    dirty_pages: &[u32],
) -> Vec<(usize, &'a [u8])> {
    use crate::render::upload::ranges::contiguous_index_runs;
    let mut updates = Vec::new();
    for run in contiguous_index_runs(dirty_records.iter().copied()) {
        let bytes = bytemuck::cast_slice(&records[run.clone()]);
        updates.push((record_base + run.start * size_of::<TileDrawRecord>(), bytes));
    }
    for run in contiguous_index_runs(dirty_pages.iter().map(|&page| page as usize)) {
        let words = run.start * TILE_DRAW_PAGE_WORDS..run.end * TILE_DRAW_PAGE_WORDS;
        let bytes = bytemuck::cast_slice(&indices[words.clone()]);
        updates.push((index_base + words.start * size_of::<u32>(), bytes));
    }
    updates
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod upload_journal_tests;

impl Scene {
    pub(crate) fn filter_candidates(
        &self,
        bounds: crate::shared::bounds::Bounds,
        plan: &ExecPlan,
    ) -> Vec<u32> {
        self.tile_draw_bins
            .draws_in_bounds(bounds, &plan.draw_order)
    }

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
            if selection.is_none() {
                return Err("fixed clip slots require a prepared clip stack".into());
            }
            plan.use_preallocated_tiles();
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
    /// Image handles must match the placements supplied to SceneCache::record.
    /// Coarse for the current draw batch must precede fine in this ComputeBatch.
    pub(crate) unsafe fn encode_fine(
        &self,
        batch: &mut ComputeBatch,
        target: ResourceId,
        images: &SceneImages,
        plan: &FinePlan,
    ) -> Result<()> {
        // SAFETY: Canvas uploads and shared stack depths bound all record/spill
        // accesses; caller supplies associated images and stage ordering.
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

#[path = "scene/clip_tiles.rs"]
mod clip_tiles;
