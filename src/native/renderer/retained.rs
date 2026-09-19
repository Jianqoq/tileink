use super::*;
use crate::{IncrementalRenderConfig, IncrementalRenderStats, RetainedScene};

impl NativeRenderer {
    pub fn render_retained(
        &mut self,
        scene: &RetainedScene,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, None)
    }

    pub fn render_retained_with_text(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, Some((fonts, text)), false, None)
    }

    pub fn render_retained_to_image(
        &mut self,
        scene: &RetainedScene,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit_retained(scene, None, true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    pub fn render_retained_to_image_with_text(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit_retained(scene, Some((fonts, text)), true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    pub fn render_retained_to_texture(
        &mut self,
        scene: &RetainedScene,
        target: &crate::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, Some(target.into()))
    }

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
    pub fn render_retained_to_target(
        &mut self,
        scene: &RetainedScene,
        target: crate::NativeRenderTarget<'_>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, None, false, Some(target))
    }
    pub fn render_retained_with_text_to_target(
        &mut self,
        scene: &RetainedScene,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
        target: crate::NativeRenderTarget<'_>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit_retained(scene, Some((fonts, text)), false, Some(target))
    }

    pub fn invalidate_retained_history(&mut self) {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        self.recording.retained.invalidate();
    }

    pub fn incremental_render_config(&self) -> IncrementalRenderConfig {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            self.recording.retained.config()
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            Default::default()
        }
    }

    pub fn set_incremental_render_config(&mut self, config: IncrementalRenderConfig) {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        self.recording.retained.set_config(config);
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        let _ = config;
    }

    pub fn incremental_render_stats(&self) -> IncrementalRenderStats {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            self.recording.retained.stats().clone()
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            Default::default()
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
    pub(crate) fn submit_retained_synchronized(
        &mut self,
        scene: &RetainedScene,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<crate::NativeRenderTarget<'_>>,
        synchronization: Option<crate::native::interop::Synchronization>,
    ) -> Result<NativeSubmission, NativeError> {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
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
            let materializer = self.persistent_scene.as_mut().unwrap();
            let unchanged = materializer.version() == scene.version();
            let changes = scene.changes_since(materializer.version());
            let changed = materializer.update(scene, changes);
            let selected = self.recording.retained.select_materialized(
                materializer.canvas(),
                unchanged || !changed,
                scene.id(),
                scene.version(),
            );
            self.submit_synchronized(selected, text, readback, output, synchronization)
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            let _ = (scene, text, readback, output, synchronization);
            Err(NativeError::Unavailable(
                self.context.backend().unavailable(),
            ))
        }
    }
}
