use super::{NativeBackend, NativeError};
use crate::Image;

/// Owns the context generation through GPU completion. Dropping it never cancels work.
#[must_use = "wait explicitly to observe completion and retire this submission"]
#[derive(Debug)]
pub struct NativeSubmission {
    backend: NativeBackend,
    #[cfg(all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    ))]
    receipt: super::runtime::adapter::Receipt,
}

impl NativeSubmission {
    pub fn backend(&self) -> NativeBackend {
        self.backend
    }
    #[cfg(all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    ))]
    pub(super) fn new(backend: NativeBackend, receipt: super::runtime::adapter::Receipt) -> Self {
        Self { backend, receipt }
    }

    /// Wait explicitly, with the backend's bounded completion wait, and retire resources.
    pub fn wait(self) -> Result<(), NativeError> {
        self.readback().map(|_| ())
    }

    fn readback(self) -> Result<Vec<Vec<u8>>, NativeError> {
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            self.receipt.readback().map_err(NativeError::Readback)
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        Err(NativeError::Unavailable(self.backend.unavailable()))
    }
}

/// A submitted explicit copy. `readback` waits and returns tightly packed premultiplied RGBA8.
#[must_use = "read back explicitly to retrieve pixels and retire this submission"]
#[derive(Debug)]
pub struct NativeImageSubmission {
    submission: NativeSubmission,
    size: (u32, u32),
}

impl NativeImageSubmission {
    pub(super) fn new(submission: NativeSubmission, size: (u32, u32)) -> Self {
        Self { submission, size }
    }

    pub fn readback(self) -> Result<Image, NativeError> {
        decode_image(self.size, self.submission.readback()?)
    }
}

fn decode_image(size: (u32, u32), mut outputs: Vec<Vec<u8>>) -> Result<Image, NativeError> {
    if outputs.len() != 1 {
        return Err(NativeError::Readback(
            "native image submission has an invalid output count".into(),
        ));
    }
    let bytes = outputs.pop().unwrap();
    let expected = (u64::from(size.0) * u64::from(size.1)).checked_mul(4);
    if expected.is_none() || u64::try_from(bytes.len()).ok() != expected {
        return Err(NativeError::Readback(
            "native image readback has an invalid byte count".into(),
        ));
    }
    Ok(Image {
        width: size.0,
        height: size.1,
        // Preserve already-premultiplied bytes; Image::from_rgba8 would multiply alpha again.
        pixels: bytes
            .chunks_exact(4)
            .map(|pixel| u32::from_le_bytes(pixel.try_into().unwrap()))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_preserves_premultiplied_and_transparent_channels_exactly() {
        let bytes = vec![70, 30, 10, 128, 0, 0, 0, 0];
        let image = decode_image((2, 1), vec![bytes.clone()]).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(bytemuck::cast_slice::<_, u8>(&image.pixels), bytes);
    }

    #[test]
    fn readback_rejects_extra_outputs_padding_and_incomplete_pixels() {
        for outputs in [
            vec![],
            vec![vec![0; 4], vec![]],
            vec![vec![0; 3]],
            vec![vec![0; 8]],
        ] {
            assert!(decode_image((1, 1), outputs).is_err());
        }
        assert!(decode_image((u32::MAX, u32::MAX), vec![vec![]]).is_err());
    }
}
