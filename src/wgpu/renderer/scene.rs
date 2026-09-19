//! GPU scene preparation and temporary local-scene resource activation.
//!
//! Offscreen filters temporarily replace most scene-bound buffers. Keeping the swap and restore
//! operations together makes that resource transaction auditable and keeps frame orchestration
//! separate from buffer preparation.

use crate::{
    TextFontSystem,
    canvas::Canvas,
    shared::{
        execution::ExecPlan,
        gpu_plan::{
            FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH, GpuBufferLengths, GpuCanvasConfig,
            plan_stack_depths,
        },
        image_resource::ImageResourceStore,
        layer::filter::Filter,
    },
    text::{PreparedTextChanges, TextContext},
};

use super::{
    super::{
        filter::WgpuFilterPipeline,
        profile::{profile_cpu, start_cpu_scope},
        target::WgpuTarget,
    },
    Renderer, SavedRendererState,
};

impl Renderer {
    pub(super) fn render_prepared_native(&mut self, canvas: &Canvas) -> bool {
        // Direct external output can prepare a larger scene without resizing owned
        // history. Allocation follows the actual output route, even after an error
        // or when unchanged retained scene preparation is reused.
        self.readback_target.resize(
            &self.device,
            canvas.physical_width(),
            canvas.physical_height(),
        );
        if self.render_prepared_tile_plan(canvas) {
            self.size = (canvas.physical_width(), canvas.physical_height());
            return true;
        }
        false
    }

    pub(super) fn prepare_scene(&mut self, canvas: &Canvas) {
        let _profile_scope = start_cpu_scope("prepare");
        self.text_data = None;
        self.prepare_scene_resources(canvas, None);
    }

    pub(super) fn prepare_scene_with_text(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        let _profile_scope = start_cpu_scope("prepare");
        let text_changes = crate::render::prepare::prepare_text(
            &mut self.text_data,
            canvas,
            font_system,
            text_context,
        );
        self.prepare_scene_resources(canvas, text_changes.as_ref());
    }

    fn prepare_scene_resources(
        &mut self,
        canvas: &Canvas,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) {
        if let Some(changes) = &canvas.buffer_changes {
            self.retained.stats_mut().chunks_rebuilt = changes.chunks_rebuilt;
            self.retained.stats_mut().plan_fragments_rebuilt = changes.plan_fragments_rebuilt;
            self.retained.stats_mut().full_scene_sync = changes.full_scene_sync;
            self.retained.stats_mut().cpu_copied_bytes = changes.cpu_copied_bytes;
            self.retained.stats_mut().arena_live_bytes = changes.arena_live_bytes;
            self.retained.stats_mut().arena_capacity_bytes = changes.arena_capacity_bytes;
            self.retained.stats_mut().arena_fragmentation = changes.arena_fragmentation;
            self.retained.stats_mut().arena_compactions = changes.arena_compactions;
        }
        self.size = (canvas.physical_width(), canvas.physical_height());
        self.surface_origin = (0, 0);
        profile_cpu("prepare.target", || {
            if self.root_target_view.is_none() {
                self.readback_target.resize(
                    &self.device,
                    canvas.physical_width(),
                    canvas.physical_height(),
                );
            }
            self.fine_portable_source.resize(
                &self.device,
                canvas.physical_width(),
                canvas.physical_height(),
            );
            self.fine_portable_target.resize(
                &self.device,
                canvas.physical_width(),
                canvas.physical_height(),
            );
        });
        let plan_metadata_profile = start_cpu_scope("prepare.plan_metadata");
        let prepared = self.scene_preparation.prepare_plan(canvas, &mut self.plan);
        let plan = &prepared.plan;
        let reused_plan_metadata = prepared.reused_metadata;
        let (max_clip_depth, max_group_depth) = prepared.stack_depths;
        let lengths = profile_cpu("prepare.lengths", || {
            self.scene_upload.build_lengths(
                canvas,
                self.text_data.as_ref(),
                plan,
                reused_plan_metadata,
                reused_plan_metadata.then_some(prepared.stack_depths),
                flat_text_changes,
            )
        });
        self.retained.stats_mut().reused_compiled_plan = reused_plan_metadata;
        if !reused_plan_metadata {
            // Like the fingerprint, this describes the outer prepared scene;
            // temporary localized scratch plans do not replace its metadata.
            self.prepared_output_read_usages = super::output::root_read_usages(plan);
        }
        drop(plan_metadata_profile);
        profile_cpu("prepare.upload_scene", || {
            self.prepare_image_resource_buffers(canvas.scene_image_resources(), false);
            self.vector_images.retain_sources(
                self.image_resource_upload
                    .vectors()
                    .iter()
                    .map(|image| &image.canvas),
            );
            let uploaded = self.scene_buffers.upload(
                &self.device,
                &self.queue,
                canvas,
                lengths,
                plan,
                self.text_data.as_ref(),
                Some(&self.image_resource_upload),
                &mut self.scene_upload,
                !reused_plan_metadata,
                flat_text_changes,
            );
            self.retained.stats_mut().gpu_uploaded_bytes += uploaded as u64;
        });
        let transient_buffers_profile = start_cpu_scope("prepare.transient_buffers");
        profile_cpu("prepare.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.coarse_buffers", || {
            profile_cpu("prepare.coarse_buffers.resize", || {
                self.coarse.prepare_outputs(&self.device, lengths);
            });
            profile_cpu("prepare.coarse_buffers.upload_tile_draw_bins", || {
                let (pages, compactions) = self.coarse.upload_tile_draw_bins(
                    &self.device,
                    &self.queue,
                    lengths,
                    &mut self.scene_upload,
                );
                self.retained.stats_mut().tile_pages_rewritten = pages as u32;
                self.retained.stats_mut().tile_page_compactions = compactions;
            });
        });
        profile_cpu("prepare.fine_spills", || {
            self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        });
        prepared.prepare_scratch(|count| self.prepare_scratch_buffers(count));
        if prepared.upload_filters {
            profile_cpu("prepare.filter_uploads", || {
                self.filter_transfers
                    .upload(&self.device, &self.queue, plan);
                self.filter_brushes.upload(
                    &self.device,
                    &self.queue,
                    plan,
                    Some(&self.image_resource_upload),
                );
                self.filter_convolves
                    .upload(&self.device, &self.queue, plan);
                self.filter_turbulence
                    .upload(&self.device, &self.queue, plan);
                self.filter_paths.upload(&self.device, &self.queue, plan);
            });
        }
        drop(transient_buffers_profile);
        profile_cpu("prepare.config", || {
            self.config.upload(
                &self.device,
                &self.queue,
                "tileink wgpu canvas config",
                &[GpuCanvasConfig::new(canvas, lengths, self.clear_color)],
            );
        });
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(prepared.plan);
    }

    pub(super) fn prepare_image_resource_buffers(
        &mut self,
        scene_resources: &ImageResourceStore,
        force_upload: bool,
    ) {
        let limits = self.device.limits();
        let max_atlas_dimension = limits.max_texture_dimension_2d;
        let max_atlas_pages = limits.max_texture_array_layers;
        let signature = self.image_resources.upload_signature(
            scene_resources,
            max_atlas_dimension,
            max_atlas_pages,
            self.image_resource_texture_table_len,
        );
        let rebuild_upload =
            self.image_resources_dirty || self.image_resource_upload_signature != signature;

        if rebuild_upload {
            self.image_resource_upload = self.image_resources.upload_merged(
                scene_resources,
                max_atlas_dimension,
                max_atlas_pages,
                self.image_resource_texture_table_len,
                Some(&self.image_resource_upload),
            );
            self.image_resource_upload_signature = signature;
            self.image_resources_dirty = false;
        }

        if force_upload || rebuild_upload {
            self.scene_buffers.upload_image_resources(
                &self.device,
                &self.queue,
                &self.image_resource_upload,
                force_upload,
            );
        }
    }

    pub(super) fn activate_local_scene_resources(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        parent_filter: &Filter,
        scratch_count: usize,
        surface_origin: (i32, i32),
    ) -> SavedRendererState {
        let _profile_scope = start_cpu_scope("prepare.local");
        let local_size = (canvas.physical_width(), canvas.physical_height());
        let local_resources = self.acquire_local_scene_resources(local_size);
        let resources = local_resources.swap_with_renderer(self);
        let saved = SavedRendererState {
            lengths: self.lengths,
            plan: self.plan.take(),
            resources,
            max_clip_depth: self.max_clip_depth,
            max_group_depth: self.max_group_depth,
            size: self.size,
            surface_origin: self.surface_origin,
            active_tiles: self.retained.take_active_tiles(),
            filter_active_tile_work: self
                .filter
                .as_ref()
                .and_then(WgpuFilterPipeline::active_tile_work),
        };

        let lengths = profile_cpu("prepare.local.lengths", || {
            self.scene_upload.build_lengths(
                canvas,
                self.text_data.as_ref(),
                plan,
                false,
                None,
                None,
            )
        });
        let (max_clip_depth, max_group_depth) =
            profile_cpu("prepare.local.stack_depths", || plan_stack_depths(plan));
        self.size = (canvas.physical_width(), canvas.physical_height());
        self.surface_origin = surface_origin;
        self.retained.set_active_tiles(None);
        if let Some(filter) = self.filter.as_mut() {
            filter.clear_active_tile_work();
        }
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.plan = Some(std::rc::Rc::new(plan.clone()));
        profile_cpu("prepare.local.targets", || {
            self.readback_target
                .resize(&self.device, local_size.0, local_size.1);
            self.fine_portable_source
                .resize(&self.device, local_size.0, local_size.1);
            self.fine_portable_target
                .resize(&self.device, local_size.0, local_size.1);
        });
        profile_cpu("prepare.local.upload_scene", || {
            self.prepare_image_resource_buffers(canvas.scene_image_resources(), true);
            self.scene_buffers.upload(
                &self.device,
                &self.queue,
                canvas,
                lengths,
                plan,
                self.text_data.as_ref(),
                Some(&self.image_resource_upload),
                &mut self.scene_upload,
                true,
                None,
            );
        });
        profile_cpu("prepare.local.scan_buffers", || {
            self.scan.prepare_outputs(&self.device, lengths);
        });
        profile_cpu("prepare.local.coarse_buffers", || {
            profile_cpu("prepare.local.coarse_buffers.resize", || {
                self.coarse.prepare_outputs(&self.device, lengths);
            });
            profile_cpu("prepare.local.coarse_buffers.upload_tile_draw_bins", || {
                self.coarse.upload_tile_draw_bins(
                    &self.device,
                    &self.queue,
                    lengths,
                    &mut self.scene_upload,
                );
            });
        });
        profile_cpu("prepare.local.fine_spills", || {
            self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        });
        profile_cpu("prepare.local.scratch", || {
            self.prepare_scratch_buffers(scratch_count.max(1));
        });
        profile_cpu("prepare.local.filter_uploads", || {
            self.filter_transfers.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_brushes.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
                Some(&self.image_resource_upload),
            );
            self.filter_convolves.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_turbulence.upload_for_ops_and_filter(
                &self.device,
                &self.queue,
                &plan.ops,
                parent_filter,
            );
            self.filter_paths.upload(&self.device, &self.queue, plan);
        });
        profile_cpu("prepare.local.config", || {
            self.config.upload(
                &self.device,
                &self.queue,
                "tileink wgpu canvas config",
                &[GpuCanvasConfig::new(canvas, lengths, self.clear_color)],
            );
        });
        saved
    }

    pub(super) fn restore_root_scene_resources(&mut self, saved: SavedRendererState) {
        let local_resources = saved.resources.swap_with_renderer(self);
        self.recycle_local_scene_resources(local_resources);
        self.lengths = saved.lengths;
        self.plan = saved.plan;
        self.max_clip_depth = saved.max_clip_depth;
        self.max_group_depth = saved.max_group_depth;
        self.size = saved.size;
        self.surface_origin = saved.surface_origin;
        self.retained.set_active_tiles(saved.active_tiles);
        if let Some(filter) = self.filter.as_mut() {
            filter.restore_active_tile_work(saved.filter_active_tile_work);
        }
    }

    pub(super) fn prepare_scratch_buffers(&mut self, count: usize) {
        while self.scratch.len() < count {
            self.scratch
                .push(WgpuTarget::new(&self.device, self.size.0, self.size.1));
        }
        for scratch in &mut self.scratch {
            scratch.resize(&self.device, self.size.0, self.size.1);
        }
        for scratch in &mut self.scratch_spares {
            scratch.resize(&self.device, self.size.0, self.size.1);
        }
        self.filter_target_snapshot
            .resize(&self.device, self.size.0, self.size.1);
        self.scratch_slots.reset(self.scratch.len());
    }

    fn prepare_fine_stack_spills(
        &mut self,
        lengths: GpuBufferLengths,
        max_clip_depth: usize,
        max_group_depth: usize,
    ) {
        let clip_depth = u32::try_from(max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH))
            .expect("bounded clip depth");
        let group_depth = u32::try_from(max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH))
            .expect("bounded group depth");
        let (_, words) = crate::render::fine::spill_layout(
            u32::try_from(lengths.tile_count).expect("bounded tile count"),
            clip_depth,
            group_depth,
        )
        .expect("bounded fine spill allocation");
        self.fine_spills
            .resize_uninit::<u32>(&self.device, "tileink wgpu fine spills", words);
    }
}
