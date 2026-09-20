use super::*;

impl NativeRenderer {
    pub(super) fn submit(
        &mut self,
        selected: crate::render::retained::SelectedScene<'_>,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<crate::NativeRenderTarget<'_>>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_synchronized(selected, text, readback, output, None)
    }
    pub(crate) fn submit_synchronized(
        &mut self,
        selected: crate::render::retained::SelectedScene<'_>,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<crate::NativeRenderTarget<'_>>,
        synchronization: Option<crate::native::interop::Synchronization>,
    ) -> Result<NativeSubmission, NativeError> {
        #[cfg(tileink_native_runtime)]
        {
            let canvas = selected.scene();
            let limits = self.context.adapter.limits();
            let size = canvas.physical_size();
            validate_size(size, limits.image_dimension)?;
            let route = super::output::OutputRoute::prepare(self, size, output)?;
            self.recording
                .retained
                .set_history_owner(route.history_owner);
            if self
                .history
                .as_ref()
                .is_none_or(|previous| !route.matches(previous))
            {
                self.recording.retained.invalidate_history();
            }
            self.recording
                .set_text_environment(text.as_ref().map(|(_, context)| &**context));
            let plan = self
                .recording
                .retained
                .begin_frame(selected.frame(), canvas, false);
            self.recording
                .retained
                .stats_mut()
                .materialized_scene_reused = selected.materialized_reused();
            if !selected.materialized_reused()
                && let Some(changes) = &canvas.buffer_changes
            {
                let stats = self.recording.retained.stats_mut();
                stats.chunks_rebuilt = changes.chunks_rebuilt;
                stats.plan_fragments_rebuilt = changes.plan_fragments_rebuilt;
                stats.full_scene_sync = changes.full_scene_sync;
                stats.cpu_copied_bytes = changes.cpu_copied_bytes;
                stats.arena_live_bytes = changes.arena_live_bytes;
                stats.arena_capacity_bytes = changes.arena_capacity_bytes;
                stats.arena_fragmentation = changes.arena_fragmentation;
                stats.arena_compactions = changes.arena_compactions;
            }
            let result = (|| {
                let mut batch = crate::native::runtime::compute::ComputeBatch::with_surfaces(
                    self.surfaces.clone(),
                );
                let output_id = batch
                    .import_texture(&route.render_target)
                    .map_err(NativeError::Recording)?;
                let target = self
                    .recording
                    .record(
                        &mut batch,
                        canvas,
                        &self.images,
                        text,
                        limits,
                        crate::native::runtime::renderer::FrameOptions {
                            target: Some(output_id),
                            ..Default::default()
                        },
                    )
                    .map_err(NativeError::Recording)?;
                route.encode_copy(&mut batch, target)?;
                if let Some(sync) = synchronization {
                    batch
                        .synchronize_target(output.expect("synchronized output").texture, sync)
                        .map_err(NativeError::Recording)?;
                }
                if readback {
                    batch.readback(target).map_err(NativeError::Recording)?;
                }
                self.recording.retained.stats_mut().gpu_uploaded_bytes = batch
                    .resources()
                    .iter()
                    .map(|resource| {
                        use crate::native::runtime::compute::Resource;
                        match resource {
                            Resource::Buffer(bytes) => bytes.len() as u64,
                            Resource::PersistentBuffer(upload) => upload.bytes.len() as u64,
                            Resource::Texture(texture) => texture.bytes.len() as u64,
                            _ => 0,
                        }
                    })
                    .sum();
                self.context.submit_compute(&batch)
            })();
            self.recording
                .retained
                .finish_frame(plan, result.is_ok(), result.is_ok());
            let submission = result?;
            let stats = self.recording.retained.stats_mut();
            stats.queue_submissions = 1;
            route.record_stats(stats);
            self.history = Some(route.capture_history());
            if route.owns_target {
                self.target = Some(route.render_target);
            }
            self.size = size;
            Ok(submission)
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = (selected, text, readback, output, synchronization);
            Err(NativeError::Unavailable(
                self.context.backend().unavailable(),
            ))
        }
    }
}
