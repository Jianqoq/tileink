//! Explicit native-backend selection. The M1 feature boundary does not pretend
//! to provide a GPU renderer before its adapter is implemented.

use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeBackend {
    Dx12,
    Vulkan,
}

impl NativeBackend {
    fn unavailable(self) -> BackendUnavailable {
        let (enabled, platform) = match self {
            Self::Dx12 => (cfg!(feature = "native-dx12"), cfg!(target_os = "windows")),
            Self::Vulkan => (
                cfg!(feature = "native-vulkan"),
                cfg!(any(target_os = "windows", target_os = "linux")),
            ),
        };
        BackendUnavailable {
            backend: self,
            reason: if !enabled {
                BackendUnavailableReason::FeatureDisabled
            } else if !platform {
                BackendUnavailableReason::UnsupportedPlatform
            } else {
                BackendUnavailableReason::AdapterNotImplemented
            },
        }
    }
}

impl fmt::Display for NativeBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dx12 => "DX12",
            Self::Vulkan => "Vulkan",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendUnavailableReason {
    FeatureDisabled,
    UnsupportedPlatform,
    AdapterNotImplemented,
}

/// A forced native choice never silently selects WGPU or another native API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendUnavailable {
    pub backend: NativeBackend,
    pub reason: BackendUnavailableReason,
}

impl fmt::Display for BackendUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            BackendUnavailableReason::FeatureDisabled => "its Cargo feature is disabled",
            BackendUnavailableReason::UnsupportedPlatform => "this platform is unsupported",
            BackendUnavailableReason::AdapterNotImplemented => {
                "its adapter is not implemented in this build"
            }
        };
        write!(f, "native {} is unavailable: {reason}", self.backend)
    }
}

impl Error for BackendUnavailable {}

mod context;
mod renderer;
mod submission;
pub use context::{NativeContext, NativeContextOptions, NativeError};
pub use renderer::NativeRenderer;
pub use submission::{NativeImageSubmission, NativeSubmission};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_backends_never_fall_back() {
        for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
            let expected = backend.unavailable();
            if expected.reason == BackendUnavailableReason::AdapterNotImplemented
                && cfg!(target_os = "windows")
            {
                continue;
            }
            let error = NativeRenderer::new(backend, 17, 19).unwrap_err();
            let NativeError::Unavailable(actual) = error else {
                panic!("unexpected initialization")
            };
            assert_eq!(actual, expected);
        }
    }
}

mod shaders;
pub use shaders::{NativeShaderArtifact, SHADER_ARTIFACTS};

#[cfg(all(
    target_os = "windows",
    any(feature = "native-dx12", feature = "native-vulkan")
))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Low-level conformance entry points are exercised by the native GPU matrix"
    )
)]
mod runtime;
