use super::{BackendUnavailable, BackendUnavailableReason, NativeBackend};
use std::{error::Error, fmt};

/// Device creation policy. An explicit physical identity never selects a different GPU.
#[derive(Clone, Debug, Default)]
pub struct NativeContextOptions {
    /// Windows LUID or Metal registry ID as sixteen lowercase hexadecimal digits;
    /// None selects a GPU. An explicit identity never falls back to another device.
    pub physical_adapter: Option<String>,
    /// Require API validation. DX12's process-wide layer must already be enabled
    /// by the host or `NativeContext::enable_dx12_validation` before device creation.
    pub validation: bool,
}

#[derive(Debug)]
pub enum NativeError {
    Unavailable(BackendUnavailable),
    Initialization(Box<dyn Error>),
    Recording(Box<dyn Error>),
    SubmissionRejected(Box<dyn Error>),
    SubmissionUnconfirmed(Box<dyn Error>),
    Completion(Box<dyn Error>),
    Readback(Box<dyn Error>),
    Validation(Box<dyn Error>),
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (stage, error): (&str, &dyn Error) = match self {
            Self::Unavailable(error) => return fmt::Display::fmt(error, f),
            Self::Initialization(error) => ("initialization", error.as_ref()),
            Self::Recording(error) => ("recording", error.as_ref()),
            Self::SubmissionRejected(error) => ("rejected submission", error.as_ref()),
            Self::SubmissionUnconfirmed(error) => ("unconfirmed submission", error.as_ref()),
            Self::Readback(error) => ("readback", error.as_ref()),
            Self::Completion(error) => ("completion", error.as_ref()),
            Self::Validation(error) => ("validation", error.as_ref()),
        };
        write!(f, "native {stage}: {error}")
    }
}

impl Error for NativeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Unavailable(error) => error,
            Self::Initialization(error)
            | Self::Recording(error)
            | Self::SubmissionRejected(error)
            | Self::SubmissionUnconfirmed(error)
            | Self::Completion(error)
            | Self::Readback(error)
            | Self::Validation(error) => error.as_ref(),
        })
    }
}

/// A shared native device and serialized queue owner. Clones preserve logical device identity.
#[derive(Clone)]
pub struct NativeContext {
    backend: NativeBackend,
    #[cfg(tileink_native_runtime)]
    pub(super) adapter: super::runtime::adapter::Adapter,
}

impl NativeContext {
    #[cfg(tileink_native_runtime)]
    pub(super) fn from_adapter(
        backend: NativeBackend,
        adapter: super::runtime::adapter::Adapter,
    ) -> Self {
        Self { backend, adapter }
    }

    #[cfg(tileink_native_runtime)]
    pub(super) fn submit_compute(
        &self,
        batch: &super::runtime::compute::ComputeBatch,
    ) -> Result<super::NativeSubmission, NativeError> {
        use crate::render::backend::SubmitError;
        self.adapter
            .submit_compute(batch)
            .map(|receipt| super::NativeSubmission::new(self.backend(), receipt))
            .map_err(|error| match error {
                SubmitError::Rejected(error) => NativeError::SubmissionRejected(error),
                SubmitError::Unconfirmed(error) => NativeError::SubmissionUnconfirmed(error),
            })
    }
    #[cfg(tileink_native_runtime)]
    pub(crate) fn create_texture_kind(
        &self,
        size: [u32; 2],
        layers: u32,
        array: bool,
    ) -> Result<super::NativeTexture, NativeError> {
        let [width, height] = size;
        if layers == 0
            || layers > self.adapter.limits().atlas_pages
            || (!array && layers != 1)
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(layers as usize))
                .and_then(|n| n.checked_mul(4))
                .is_none()
        {
            return Err(NativeError::Recording(
                "invalid native texture layers or byte extent".into(),
            ));
        }
        super::renderer::validate_size((width, height), self.adapter.limits().image_dimension)?;
        Ok(super::NativeTexture {
            state: std::rc::Rc::new(super::runtime::texture::State {
                allocation: self
                    .adapter
                    .allocate_texture([width, height], layers, array)
                    .map_err(NativeError::Initialization)?,
                initialized: std::cell::Cell::new(false),
                content_version: std::cell::Cell::new(0),
            }),
            context: self.clone(),
            size: [width, height],
            layers,
            array,
        })
    }

    /// Allocate a persistent RGBA8 target. Its first submitted use clears it on
    /// the GPU; subsequent submissions preserve untouched pixels without upload.
    pub fn create_texture(
        &self,
        width: u32,
        height: u32,
    ) -> Result<super::NativeTexture, NativeError> {
        #[cfg(tileink_native_runtime)]
        {
            self.create_texture_kind([width, height], 1, false)
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = (width, height);
            Err(NativeError::Unavailable(self.backend.unavailable()))
        }
    }
    /// Enable the process-wide DX12 layer before creating devices. Repeated calls
    /// after a successful call are no-ops; late Tileink-owned creation is rejected.
    ///
    /// # Safety
    /// Before the first successful call, no DX12 device created outside Tileink may
    /// exist, and external device creation must not run concurrently with this call.
    pub unsafe fn enable_dx12_validation() -> Result<(), NativeError> {
        #[cfg(all(target_os = "windows", feature = "dx12"))]
        {
            unsafe { super::runtime::enable_dx12_validation().map_err(NativeError::Initialization) }
        }
        #[cfg(not(all(target_os = "windows", feature = "dx12")))]
        {
            Err(NativeError::Unavailable(NativeBackend::Dx12.unavailable()))
        }
    }
    pub fn new(
        backend: NativeBackend,
        options: &NativeContextOptions,
    ) -> Result<Self, NativeError> {
        let unavailable = backend.unavailable();
        if unavailable.reason != BackendUnavailableReason::AdapterNotImplemented {
            return Err(NativeError::Unavailable(unavailable));
        }
        #[cfg(tileink_native_runtime)]
        {
            let adapter = super::runtime::adapter::Adapter::with_options(backend, options)
                .map_err(NativeError::Initialization)?;
            validate_texture_table_capacity(adapter.limits().texture_table_len)?;
            Ok(Self { backend, adapter })
        }
        #[cfg(not(tileink_native_runtime))]
        {
            let _ = options;
            Err(NativeError::Unavailable(unavailable))
        }
    }

    pub fn backend(&self) -> NativeBackend {
        self.backend
    }

    /// Check enabled native validation after completion. The known DX12 optimized-
    /// clear advisory from coexisting wgpu rendering is reported but is nonfatal;
    /// its error-severity form and all correctness warnings/errors still fail.
    pub fn check_validation(&self) -> Result<(), NativeError> {
        #[cfg(tileink_native_runtime)]
        {
            self.adapter
                .assert_valid_with_wgpu_clears()
                .map_err(NativeError::Validation)
        }
        #[cfg(not(tileink_native_runtime))]
        {
            Err(NativeError::Unavailable(self.backend.unavailable()))
        }
    }
}

#[cfg(any(test, tileink_native_runtime))]
fn validate_texture_table_capacity(capacity: u32) -> Result<(), NativeError> {
    let required = crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY;
    if capacity < required {
        return Err(NativeError::Initialization(
            format!(
                "native renderer requires a non-uniform sampled-image table with {required} entries"
            )
            .into(),
        ));
    }
    Ok(())
}

impl fmt::Debug for NativeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeContext")
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_texture_table_support_is_rejected_before_a_renderer_exists() {
        let required = crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY;
        assert!(validate_texture_table_capacity(0).is_err());
        assert!(validate_texture_table_capacity(required - 1).is_err());
        assert!(validate_texture_table_capacity(required).is_ok());
    }
}
