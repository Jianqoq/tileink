use super::submission::{NativeImageSubmission, NativeSubmission};
use super::{NativeBackend, NativeContext, NativeContextOptions, NativeError};
use crate::shared::image_resource::ImageResourceStore;
use crate::{Canvas, Image, ImageKey, TextContext, TextFontSystem};
use std::rc::Rc;

/// Immediate native Canvas renderer. Scene and text caches are local to this renderer.
pub struct NativeRenderer {
    context: NativeContext,
    size: (u32, u32),
    images: ImageResourceStore,
    #[cfg(all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    ))]
    recording: super::runtime::renderer::recording::Recording,
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
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            validate_size((width, height), context.adapter.limits().image_dimension)?;
            Ok(Self {
                context: context.clone(),
                size: (width, height),
                images: ImageResourceStore::default(),
                recording: Default::default(),
            })
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
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
    pub fn insert_image(&mut self, key: ImageKey, image: impl Into<Rc<Image>>) -> bool {
        self.images.insert(key, image.into())
    }
    pub fn remove_image(&mut self, key: ImageKey) -> bool {
        self.images.remove(key)
    }
    pub fn clear_images(&mut self) -> bool {
        self.images.clear()
    }

    /// Submit without a CPU wait or readback copy. Explicitly wait on the returned receipt.
    pub fn render(&mut self, canvas: &Canvas) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, None, false)
    }
    pub fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, Some((fonts, text)), false)
    }
    /// Submit rendering and an explicit image readback copy; mapping/waiting is separate.
    pub fn render_to_image(
        &mut self,
        canvas: &Canvas,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(canvas, None, true)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }
    pub fn render_to_image_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(canvas, Some((fonts, text)), true)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    fn submit(
        &mut self,
        canvas: &Canvas,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
    ) -> Result<NativeSubmission, NativeError> {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            use crate::render::backend::SubmitError;
            let limits = self.context.adapter.limits();
            let size = canvas.physical_size();
            validate_size(size, limits.image_dimension)?;
            let mut batch = super::runtime::compute::ComputeBatch::new();
            let target = self
                .recording
                .record(&mut batch, canvas, &self.images, text, limits, false)
                .map_err(NativeError::Recording)?;
            if readback {
                batch.readback(target).map_err(NativeError::Recording)?;
            }
            let receipt =
                self.context
                    .adapter
                    .submit_compute(&batch)
                    .map_err(|error| match error {
                        SubmitError::Rejected(error) => NativeError::SubmissionRejected(error),
                        SubmitError::Unconfirmed(error) => {
                            NativeError::SubmissionUnconfirmed(error)
                        }
                    })?;
            self.size = size;
            Ok(NativeSubmission::new(self.context.backend(), receipt))
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            let _ = (canvas, text, readback);
            Err(NativeError::Unavailable(
                self.context.backend().unavailable(),
            ))
        }
    }
}

#[cfg(any(
    test,
    all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    )
))]
fn validate_size(size: (u32, u32), max_dimension: u32) -> Result<(), NativeError> {
    if size.0 == 0 || size.1 == 0 || size.0 > max_dimension || size.1 > max_dimension {
        return Err(NativeError::Recording(
            "native image dimensions exceed device limits or are empty".into(),
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
        assert!(validate_size((17, 3), 17).is_ok());
        assert!(validate_size((3, 17), 17).is_ok());
        for size in [(0, 3), (3, 0), (18, 3), (3, 18)] {
            assert!(validate_size(size, 17).is_err());
        }
    }
}
