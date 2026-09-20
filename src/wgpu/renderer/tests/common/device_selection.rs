//! CPU-only selection contract for the GPU test harness.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TestApi {
    Default,
    Dx12,
    Vulkan,
    Metal,
}

impl TestApi {
    pub(super) fn parse(value: Option<&str>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Default),
            Some("dx12") => Ok(Self::Dx12),
            Some("vulkan") => Ok(Self::Vulkan),
            Some("metal") => Ok(Self::Metal),
            Some(_) => Err("TILEINK_TEST_API must be dx12, vulkan, or metal when supplied"),
        }
    }

    pub(super) fn explicit_name(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Dx12 => Some("dx12"),
            Self::Vulkan => Some("vulkan"),
            Self::Metal => Some("metal"),
        }
    }

    pub(super) fn slot(self, portable: bool) -> usize {
        self as usize * 2 + usize::from(portable)
    }
}

#[test]
fn every_api_and_texture_mode_has_an_independent_cached_device() {
    let mut slots = Vec::new();
    for api in [
        TestApi::Default,
        TestApi::Dx12,
        TestApi::Vulkan,
        TestApi::Metal,
    ] {
        for portable in [false, true] {
            slots.push(api.slot(portable));
        }
    }
    slots.sort_unstable();
    assert_eq!(slots, [0, 1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn an_explicit_request_never_means_default_selection() {
    assert_eq!(
        TestApi::parse(Some("metal")).unwrap().explicit_name(),
        Some("metal")
    );
    assert_eq!(TestApi::parse(None).unwrap().explicit_name(), None);
    assert_eq!(
        TestApi::parse(Some("dx12")).unwrap().explicit_name(),
        Some("dx12")
    );
    assert_eq!(
        TestApi::parse(Some("vulkan")).unwrap().explicit_name(),
        Some("vulkan")
    );
}

#[test]
fn unknown_or_empty_requests_are_rejected() {
    for value in ["", "auto", "native", "portable", "DX12", "METAL"] {
        assert!(TestApi::parse(Some(value)).is_err());
    }
}

#[test]
fn whole_canvas_helper_rejects_invalid_api_before_device_creation() {
    // A child process gives this harness regression its own environment. Mutating
    // environment variables inside a multi-threaded Rust process would be unsafe.
    const PROBE: &str = "TILEINK_TEST_SELECTION_CHILD";
    if std::env::var(PROBE).as_deref() != Ok("whole-canvas") {
        let qualified = concat!(
            module_path!(),
            "::whole_canvas_helper_rejects_invalid_api_before_device_creation"
        );
        let test = qualified.split_once("::").unwrap().1;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(PROBE, "whole-canvas")
            .env("TILEINK_TEST_API", "invalid-api-probe")
            .output()
            .expect("run isolated API selection probe");
        assert!(
            output.status.success(),
            "API selection bypass: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "the subprocess must execute exactly the requested regression"
        );
        return;
    }
    let panic = std::panic::catch_unwind(|| {
        super::render_native_wgpu(&crate::Canvas::new(1, 1, 1.0));
    })
    .expect_err("explicit invalid API must be rejected by the whole-canvas helper");
    let text = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        text.contains("invalid explicit GPU test API"),
        "unexpected failure: {text}"
    );
}
