use super::{BackendUnavailable, BackendUnavailableReason, NativeBackend};
use std::{error::Error, fmt};

/// Device creation policy. An explicit physical identity never selects a different GPU.
#[derive(Clone, Debug, Default)]
pub struct NativeContextOptions {
    /// Windows adapter LUID as sixteen lowercase hexadecimal digits; None selects a GPU.
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
            | Self::Readback(error)
            | Self::Validation(error) => error.as_ref(),
        })
    }
}

/// A shared native device and serialized queue owner. Clones preserve logical device identity.
#[derive(Clone)]
pub struct NativeContext {
    backend: NativeBackend,
    #[cfg(all(
        target_os = "windows",
        any(feature = "native-dx12", feature = "native-vulkan")
    ))]
    pub(super) adapter: super::runtime::adapter::Adapter,
}

impl NativeContext {
    /// Enable the process-wide DX12 layer before creating devices. Repeated calls
    /// after a successful call are no-ops; late Tileink-owned creation is rejected.
    ///
    /// # Safety
    /// Before the first successful call, no DX12 device created outside Tileink may
    /// exist, and external device creation must not run concurrently with this call.
    pub unsafe fn enable_dx12_validation() -> Result<(), NativeError> {
        #[cfg(all(target_os = "windows", feature = "native-dx12"))]
        {
            unsafe { super::runtime::enable_dx12_validation().map_err(NativeError::Initialization) }
        }
        #[cfg(not(all(target_os = "windows", feature = "native-dx12")))]
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
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            let adapter = super::runtime::adapter::Adapter::with_options(backend, options)
                .map_err(NativeError::Initialization)?;
            validate_texture_table_capacity(adapter.limits().texture_table_len)?;
            Ok(Self { backend, adapter })
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
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
        #[cfg(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        ))]
        {
            self.adapter
                .assert_valid_with_wgpu_clears()
                .map_err(NativeError::Validation)
        }
        #[cfg(not(all(
            target_os = "windows",
            any(feature = "native-dx12", feature = "native-vulkan")
        )))]
        {
            Err(NativeError::Unavailable(self.backend.unavailable()))
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
