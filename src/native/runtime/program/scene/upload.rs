//! Upload scene inputs and assemble scan/coarse/fine bindings.
use super::super::{resources::allocate, scene_scan::PreparedScan};
use super::*;
use crate::render::{coarse::validate_work_layout, fine::spill_layout, upload::paint::PaintData};
use crate::shared::{
    gpu_coarse::CoarseChunkRecord,
    gpu_plan::{FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH},
};

impl SceneCache {
    pub(super) fn record_prepared(
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
        let custom = prepared
            .has_programs
            .then(|| crate::native::runtime::program_shader::prepare_brushes(batch, canvas, images))
            .transpose()?;
        let draws = if let Some(custom) = &custom {
            // These remapped offsets are frame-local. Never install them into
            // the retained dirty-journal cache of original Canvas records.
            self.buffers.draws = Default::default();
            self.buffers.paint = Default::default();
            batch.buffer(bytemuck::cast_slice(&custom.draws).to_vec())?
        } else {
            self.buffers.draws.upload(
                batch,
                &canvas.draw_records,
                canvas.buffer_changes.as_ref().map(|c| c.draws.as_slice()),
            )?
        };
        let paint = self.staging.paint.prepare(canvas, images);
        let paint_offsets = (paint.shadow_base, paint.brush_base);
        let paint = if let Some(custom) = &custom {
            // Repack native paints as ordinary GPU image records. Shader source
            // stays on the CPU and is never uploaded as interpreted paint data.
            let mut words = vec![0; paint_offsets.1 as usize];
            words[..canvas.sdf_blob.len()].copy_from_slice(&canvas.sdf_blob);
            words
                [paint_offsets.0 as usize..paint_offsets.0 as usize + canvas.sdf_shadow_blob.len()]
                .copy_from_slice(&canvas.sdf_shadow_blob);
            words.extend_from_slice(&custom.blob);
            batch.buffer(bytemuck::cast_slice(&words).to_vec())?
        } else {
            match paint.data {
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
        let layer_ranges = canvas
            .buffer_changes
            .as_ref()
            .filter(|_| prepared.reused_metadata)
            .map(|changes| changes.plan_layer_stack.as_slice());
        let full_layers = self
            .staging
            .refresh_layer_stack(&prepared.plan.layer_stack_data, layer_ranges);
        let layers = self.buffers.layers.upload(
            batch,
            &self.staging.layer_stack,
            if full_layers { None } else { layer_ranges },
        )?;
        let batch_ranges = canvas.buffer_changes.as_ref().map(|changes| {
            crate::render::upload::ranges::merge_sorted_dirty_ranges(
                &changes.draws,
                &changes.painter,
            )
        });
        let batch_ids = canvas.raster_batch_ids(&prepared.plan);
        let batch_dirty = canvas.retained_painter().and(batch_ranges.as_deref());
        let batches = self.buffers.batches.upload(batch, batch_ids, batch_dirty)?;
        let active_batches = if let Some(active) = options.active {
            if active.dimensions() != (lengths.tiles_width as u32, lengths.tiles_height as u32) {
                return Err("native damage dimensions differ from scene".into());
            }
            self.staging
                .active_batch_ids(active, canvas, &prepared.plan)
        } else {
            Vec::new()
        };
        let (work, clip_dispatch) = self.upload_work(
            batch,
            canvas,
            &prepared,
            lengths,
            work_words,
            options.active,
        )?;
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
            program_images: custom.map(|c| c.images),
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
