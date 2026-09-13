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

/// Native construction entry for the feature-split milestone.
///
/// No native renderer exists yet: construction reports the requested backend's
/// precise unavailable reason. The empty type prevents a partially initialized
/// instance from looking like a working renderer.
#[derive(Debug)]
pub enum NativeRenderer {}

impl NativeRenderer {
    pub fn new(
        backend: NativeBackend,
        _width: u32,
        _height: u32,
    ) -> Result<Self, BackendUnavailable> {
        Err(backend.unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forced_backend_never_falls_back_or_returns_a_renderer() {
        for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
            let error = NativeRenderer::new(backend, 17, 19).unwrap_err();
            assert_eq!(error.backend, backend);
            assert!(error.to_string().contains(&backend.to_string()));
        }
    }

    #[cfg(not(feature = "native-dx12"))]
    #[test]
    fn requesting_disabled_dx12_reports_its_feature() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Dx12, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::FeatureDisabled
        );
    }

    #[cfg(not(feature = "native-vulkan"))]
    #[test]
    fn requesting_disabled_vulkan_reports_its_feature() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Vulkan, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::FeatureDisabled
        );
    }

    #[cfg(all(feature = "native-dx12", not(target_os = "windows")))]
    #[test]
    fn dx12_is_unavailable_on_other_platforms() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Dx12, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::UnsupportedPlatform
        );
    }

    #[cfg(all(
        feature = "native-vulkan",
        not(any(target_os = "windows", target_os = "linux"))
    ))]
    #[test]
    fn vulkan_is_unavailable_outside_its_supported_platforms() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Vulkan, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::UnsupportedPlatform
        );
    }

    #[cfg(all(feature = "native-dx12", target_os = "windows"))]
    #[test]
    fn enabled_dx12_still_reports_the_unimplemented_adapter() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Dx12, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::AdapterNotImplemented
        );
    }

    #[cfg(all(
        feature = "native-vulkan",
        any(target_os = "windows", target_os = "linux")
    ))]
    #[test]
    fn enabled_vulkan_still_reports_the_unimplemented_adapter() {
        assert_eq!(
            NativeRenderer::new(NativeBackend::Vulkan, 17, 19)
                .unwrap_err()
                .reason,
            BackendUnavailableReason::AdapterNotImplemented
        );
    }
}
