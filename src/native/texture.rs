use super::{NativeBackend, NativeContext};

/// A persistent, single-mip premultiplied RGBA8 GPU allocation.
/// Clones refer to the same image and logical device, not copied pixels.
#[derive(Clone)]
pub struct NativeTexture {
    // Allocation drops before its last device owner. Submitted frames separately
    // retain the raw allocation without creating a context/queue ownership cycle.
    #[cfg(all(target_os = "windows", any(feature = "dx12", feature = "vulkan")))]
    pub(super) state: std::rc::Rc<super::runtime::texture::State>,
    pub(super) context: NativeContext,
    pub(super) size: [u32; 2],
    pub(super) layers: u32,
    pub(super) array: bool,
}

impl NativeTexture {
    /// Submit an explicit GPU readback copy. The returned receipt waits/maps only
    /// when its `readback` method is called; normal rendering does not read pixels.
    pub fn readback(&self) -> Result<super::NativeImageSubmission, super::NativeError> {
        if self.array {
            return Err(super::NativeError::Recording(
                "array texture is not a two-dimensional output image".into(),
            ));
        }

        #[cfg(all(target_os = "windows", any(feature = "dx12", feature = "vulkan")))]
        {
            let mut batch = super::runtime::compute::ComputeBatch::new();
            let image = batch
                .import_texture(self)
                .map_err(super::NativeError::Recording)?;
            batch
                .readback(image)
                .map_err(super::NativeError::Recording)?;
            Ok(super::NativeImageSubmission::new(
                self.context.submit_compute(&batch)?,
                self.size(),
            ))
        }
        #[cfg(not(all(target_os = "windows", any(feature = "dx12", feature = "vulkan"))))]
        {
            Err(super::NativeError::Unavailable(
                self.backend().unavailable(),
            ))
        }
    }
    pub fn size(&self) -> (u32, u32) {
        (self.size[0], self.size[1])
    }
    pub fn backend(&self) -> NativeBackend {
        self.context.backend()
    }
}

impl std::fmt::Debug for NativeTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTexture")
            .field("backend", &self.backend())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}
