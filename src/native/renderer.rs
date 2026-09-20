mod retained;
mod submission;

use super::submission::{NativeImageSubmission, NativeSubmission};
use super::{NativeBackend, NativeContext, NativeContextOptions, NativeError};
use crate::shared::image_resource::ImageResourceStore;
use crate::{Canvas, Image, ImageKey, TextContext, TextFontSystem};
use std::rc::Rc;

/// Immediate and retained native renderer. Scene and text caches are local to this renderer.
pub struct NativeRenderer {
    context: NativeContext,
    size: (u32, u32),
    images: ImageResourceStore,
    #[cfg(tileink_native_runtime)]
    target: Option<super::NativeTexture>,
    #[cfg(tileink_native_runtime)]
    surfaces: Rc<std::cell::RefCell<super::runtime::compute::SurfacePool>>,
    #[cfg(tileink_native_runtime)]
    recording: super::runtime::renderer::recording::Recording,
    #[cfg(tileink_native_runtime)]
    persistent_scene: Option<crate::retained_scene::PersistentSceneMaterializer>,
    #[cfg(tileink_native_runtime)]
    history: Option<output::HistoryRecord>,
}

impl NativeRenderer {
    pub fn new(backend: NativeBackend, width: u32, height: u32) -> Result<Self, NativeError> {
        Self::with_context(
            &NativeContext::new(backend, &NativeContextOptions::default())?,
            width,
            height,
        )
    }

    pub fn with_context(
        context: &NativeContext,
        width: u32,
        height: u32,
    ) -> Result<Self, NativeError> {
        #[cfg(tileink_native_runtime)]
        {
            validate_size((width, height), context.adapter.limits().image_dimension)?;
            Ok(Self {
                context: context.clone(),
                size: (width, height),
                images: ImageResourceStore::default(),
                target: None,
                surfaces: Rc::new(std::cell::RefCell::new(
                    super::runtime::compute::SurfacePool::new(context),
                )),
                recording: Default::default(),
                persistent_scene: None,
                history: None,
            })
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = (width, height);
            Err(NativeError::Unavailable(context.backend().unavailable()))
        }
    }

    pub fn context(&self) -> &NativeContext {
        &self.context
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    /// Inspect a registered raster image without submitting GPU work. Hosts use this
    /// to populate independent recording destinations without repeating decoded uploads.
    pub fn image_resource(&self, key: ImageKey) -> Option<&Image> {
        self.images.get(key)
    }

    pub fn insert_image(&mut self, key: ImageKey, image: impl Into<Rc<Image>>) -> bool {
        let changed = self.images.insert(key, image.into());
        if changed {
            self.invalidate_retained_history();
        }
        changed
    }
    pub fn remove_image(&mut self, key: ImageKey) -> bool {
        let changed = self.images.remove(key);
        if changed {
            self.invalidate_retained_history();
        }
        changed
    }
    /// Set the premultiplied background for subsequent root frames. Child canvases
    /// and filter intermediates retain transparent initial contents.
    pub fn set_clear_color(&mut self, clear: peniko::Color) {
        #[cfg(tileink_native_runtime)]
        {
            let clear = crate::shared::image::premul_color_to_rgba8_pack(clear);
            if self.recording.clear_color != clear {
                self.recording.clear_color = clear;
                self.invalidate_retained_history();
            }
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = clear;
        }
    }

    pub fn clear_images(&mut self) -> bool {
        let changed = self.images.clear();
        if changed {
            self.invalidate_retained_history();
        }
        changed
    }

    /// Submit without a CPU wait or readback copy. Explicitly wait on the returned receipt.
    pub fn render(&mut self, canvas: &Canvas) -> Result<NativeSubmission, NativeError> {
        self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            None,
            false,
            None,
        )
    }
    pub fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            Some((fonts, text)),
            false,
            None,
        )
    }
    /// Submit rendering and an explicit image readback copy; mapping/waiting is separate.
    pub fn render_to_image(
        &mut self,
        canvas: &Canvas,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            None,
            true,
            None,
        )?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }
    pub fn render_to_image_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            Some((fonts, text)),
            true,
            None,
        )?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    /// Render an immediate frame into a target rectangle without CPU readback.
    pub fn render_to_target(
        &mut self,
        canvas: &Canvas,
        target: crate::NativeRenderTarget<'_>,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            None,
            false,
            Some(target),
        )
    }

    /// Render into an existing same-device RGBA8 target, replacing the full frame.
    /// Queue ordering preserves dependencies without a CPU wait or pixel upload.
    pub fn render_to_texture(
        &mut self,
        canvas: &Canvas,
        target: &super::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            None,
            false,
            Some(target.into()),
        )
    }
    pub fn render_with_text_to_texture(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
        target: &super::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            Some((fonts, text)),
            false,
            Some(target.into()),
        )
    }
}

#[cfg(any(test, tileink_native_runtime))]
pub(super) fn validate_size(size: (u32, u32), max_dimension: u32) -> Result<(), NativeError> {
    if size.0 == 0 || size.1 == 0 || size.0 > max_dimension || size.1 > max_dimension {
        return Err(NativeError::Recording(
            "native image dimensions exceed device limits or are empty".into(),
        ));
    }
    // Every target can be explicitly read back as tightly packed RGBA8. Reject
    // unrepresentable allocations before GPU allocation or host size arithmetic.
    if (size.0 as usize)
        .checked_mul(size.1 as usize)
        .and_then(|n| n.checked_mul(4))
        .is_none()
    {
        return Err(NativeError::Recording(
            "native image byte size overflow".into(),
        ));
    }
    Ok(())
}

impl std::fmt::Debug for NativeRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRenderer")
            .field("context", &self.context)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_extent_is_checked_independently_on_both_axes() {
        assert!(validate_size((u32::MAX, u32::MAX), u32::MAX).is_err());
        assert!(validate_size((17, 3), 17).is_ok());
        assert!(validate_size((3, 17), 17).is_ok());
        for size in [(0, 3), (3, 0), (18, 3), (3, 18)] {
            assert!(validate_size(size, 17).is_err());
        }
    }
}

#[cfg(tileink_native_runtime)]
mod output;
