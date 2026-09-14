use super::*;

impl Renderer {
    /// Resolve child scenes on this device and append their work to the parent's
    /// command batch. No image readback, extra device or per-child submission is used.
    pub(super) fn encode_vector_images(&mut self, commands: &mut WgpuCommandBatch) -> bool {
        let Some(upload) = self.scene_buffers.vector_image_upload() else {
            return true;
        };
        if commands.resource_writes.available(&upload.ready) {
            return true;
        }
        let _profile = start_cpu_scope("prepare.vector_images");
        for request in &upload.requests {
            let source = request
                .source
                .upgrade()
                .expect("prepared vector image belongs to the live source graph");
            let entry = self.vector_images.get_or_insert(&source, || {
                let (width, height) = source.physical_size();
                Renderer::new_with_options(
                    &self.device,
                    &self.queue,
                    width,
                    height,
                    Color::TRANSPARENT,
                    self.options.clone(),
                )
            });
            if !commands.resource_writes.available(&entry.ready) {
                entry.value.prepare_scene(&source);
                if !entry
                    .value
                    .encode_prepared_tile_plan(commands, &source, None, false)
                {
                    return false;
                }
                commands.resource_writes.record(&entry.ready);
            }
            self.scene_buffers.copy_vector_image(
                commands,
                entry.value.readback_target.texture(),
                request.placement,
            );
        }
        commands.resource_writes.record(&upload.ready);
        true
    }
}
