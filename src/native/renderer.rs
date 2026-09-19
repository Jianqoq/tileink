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
    target: Option<super::NativeTexture>,
    #[cfg(all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    ))]
    surfaces: Rc<std::cell::RefCell<super::runtime::compute::SurfacePool>>,
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
                target: None,
                surfaces: Rc::new(std::cell::RefCell::new(
                    super::runtime::compute::SurfacePool::new(context),
                )),
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
    /// Set the premultiplied background for subsequent root frames. Child canvases
    /// and filter intermediates retain transparent initial contents.
    pub fn set_clear_color(&mut self, clear: peniko::Color) {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            self.recording.clear_color = crate::shared::image::premul_color_to_rgba8_pack(clear);
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            let _ = clear;
        }
    }

    pub fn clear_images(&mut self) -> bool {
        self.images.clear()
    }

    /// Submit without a CPU wait or readback copy. Explicitly wait on the returned receipt.
    pub fn render(&mut self, canvas: &Canvas) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, None, false, None)
    }
    pub fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, Some((fonts, text)), false, None)
    }
    /// Submit rendering and an explicit image readback copy; mapping/waiting is separate.
    pub fn render_to_image(
        &mut self,
        canvas: &Canvas,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(canvas, None, true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }
    pub fn render_to_image_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<NativeImageSubmission, NativeError> {
        let submission = self.submit(canvas, Some((fonts, text)), true, None)?;
        Ok(NativeImageSubmission::new(submission, self.size))
    }

    /// Render into an existing same-device RGBA8 target, replacing the full frame.
    /// Queue ordering preserves dependencies without a CPU wait or pixel upload.
    pub fn render_to_texture(
        &mut self,
        canvas: &Canvas,
        target: &super::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, None, false, Some(target))
    }
    pub fn render_with_text_to_texture(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
        target: &super::NativeTexture,
    ) -> Result<NativeSubmission, NativeError> {
        self.submit(canvas, Some((fonts, text)), false, Some(target))
    }

    fn submit(
        &mut self,
        canvas: &Canvas,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        readback: bool,
        output: Option<&super::NativeTexture>,
    ) -> Result<NativeSubmission, NativeError> {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            let limits = self.context.adapter.limits();
            let size = canvas.physical_size();
            validate_size(size, limits.image_dimension)?;
            if let Some(output) = output {
                if output.size() != size {
                    return Err(NativeError::Recording(
                        "native output size differs from canvas".into(),
                    ));
                }
                if !self.context.adapter.same_device(&output.context.adapter) {
                    return Err(NativeError::Recording(
                        "native output belongs to another logical device".into(),
                    ));
                }
            }
            // Reuse the GPU root allocation across ordered frames. Resize publishes
            // its replacement only after successful submission; old receipts keep
            // any still-running allocation alive independently of this renderer.
            let owned_target = if output.is_none() {
                Some(match &self.target {
                    Some(target) if target.size() == size => target.clone(),
                    _ => self.context.create_texture(size.0, size.1)?,
                })
            } else {
                None
            };
            let output = output.or(owned_target.as_ref());
            let mut batch =
                super::runtime::compute::ComputeBatch::with_surfaces(self.surfaces.clone());
            let output = output
                .map(|target| batch.import_texture(target))
                .transpose()
                .map_err(NativeError::Recording)?;
            let target = self
                .recording
                .record(
                    &mut batch,
                    canvas,
                    &self.images,
                    text,
                    limits,
                    super::runtime::renderer::FrameOptions {
                        target: output,
                        ..Default::default()
                    },
                )
                .map_err(NativeError::Recording)?;
            if readback {
                batch.readback(target).map_err(NativeError::Recording)?;
            }
            let submission = self.context.submit_compute(&batch)?;
            if let Some(target) = owned_target {
                self.target = Some(target);
            }
            self.size = size;
            Ok(submission)
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            let _ = (canvas, text, readback, output);
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
