use super::*;
use crate::{IncrementalRenderConfig, IncrementalRenderStats, RetainedScene};

#[cfg_attr(
    not(tileink_native_runtime),
    allow(
        clippy::missing_const_for_fn,
        clippy::unused_self,
        clippy::needless_pass_by_ref_mut,
        reason = "Unavailable-backend stubs retain the runtime API; native implementations are checked on Windows/macOS"
    )
)]
impl NativeRenderer {
    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained(
        &mut self,
        scene: &RetainedScene,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, None)
    }

    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_with_text(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, Some((fonts, text)), false, None)
    }

    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_to_image(
        &mut self,
        scene: &RetainedScene,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit_retained(scene, None, true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_to_image_with_text(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit_retained(scene, Some((fonts, text)), true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_to_texture(
        &mut self,
        scene: &RetainedScene,
        target: &crate::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, Some(target.into()))
    }

    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_with_text_to_texture(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
        target: &crate::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, Some((fonts, text)), false, Some(target.into()))
    }

    /// Render a retained frame into a target with explicit history and origin.
    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_to_target(
        &mut self,
        scene: &RetainedScene,
        target: crate::NativeRenderTarget<'_>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, Some(target))
    }
    ///
    /// # Errors
    /// Returns an error for invalid scene/target resources, shader compilation/encoding failures, or rejected/unconfirmed native queue submission.
    pub fn render_retained_with_text_to_target(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
        target: crate::NativeRenderTarget<'_>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, Some((fonts, text)), false, Some(target))
    }

    pub const fn invalidate_retained_history(&mut self) {
        #[cfg(tileink_native_runtime)]
        self.recording.retained.invalidate();
    }

    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "The backend-free fallback constructs Default at runtime"
    )]
    pub fn incremental_render_config(&self) -> IncrementalRenderConfig {
        #[cfg(tileink_native_runtime)]
        {
            self.recording.retained.config()
        }
        #[cfg(not(tileink_native_runtime))]
        {
            IncrementalRenderConfig::default()
        }
    }

    pub fn set_incremental_render_config(&mut self, config: IncrementalRenderConfig) {
        #[cfg(tileink_native_runtime)]
        self.recording.retained.set_config(config);
        #[cfg(not(tileink_native_runtime))]
        let _ = config;
    }

    #[must_use]
    pub fn incremental_render_stats(&self) -> IncrementalRenderStats {
        #[cfg(tileink_native_runtime)]
        {
            self.recording.retained.stats().clone()
        }
        #[cfg(not(tileink_native_runtime))]
        {
            IncrementalRenderStats::default()
        }
    }

    fn submit_retained(
        &mut self,
        scene: &RetainedScene,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<crate::NativeRenderTarget<'_>>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained_synchronized(scene, text, readback, output, None)
    }
    #[allow(
        clippy::expect_used,
        reason = "The matching materializer is installed before retained submission preparation"
    )]
    pub(crate) fn submit_retained_synchronized(
        &mut self,
        scene: &RetainedScene,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<crate::NativeRenderTarget<'_>>,
        synchronization: Option<crate::native::interop::Synchronization>,
    ) -> Result<NativeSubmission, NativeError> {
        #[cfg(tileink_native_runtime)]
        {
            if self
                .persistent_scene
                .as_ref()
                .is_none_or(|cached| cached.scene_id() != scene.id())
            {
                self.recording.retained.invalidate();
                self.persistent_scene = Some(
                    crate::retained_scene::PersistentSceneMaterializer::new(scene),
                );
            }
            let materializer = self.persistent_scene.as_mut().expect(
                "The matching materializer is installed before retained submission preparation",
            );
            let unchanged = materializer.version() == scene.version();
            let changes = scene.changes_since(materializer.version());
            let plan_may_change = !unchanged
                && changes
                    .as_ref()
                    .is_none_or(|changes| changes.topology_changed || changes.surface_changed);
            if plan_may_change {
                // The recording cache otherwise keeps a second Rc to the previous plan.
                // A single layer edit would then copy the entire plan in materializer.update.
                self.recording.release_retained_plan();
            }
            let changed = materializer.update(scene, changes);
            let canvas = materializer.canvas();
            if plan_may_change {
                // Restore the patched plan after the materializer has unique ownership.
                // Preparation can then retain its stack-depth metadata for unchanged structure.
                self.recording.install_retained_plan(&canvas);
            }
            let selected = self.recording.retained.select_materialized(
                canvas,
                unchanged || !changed,
                scene.id(),
                scene.version(),
            );
            self.submit_synchronized(selected, text, readback, output, synchronization)
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = (scene, text, readback, output, synchronization);
            Err(NativeError::Unavailable(
                self.context.backend().unavailable(),
            ))
        }
    }
}
