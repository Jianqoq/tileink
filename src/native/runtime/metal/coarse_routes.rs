//! Pinned Metal adapter for shared coarse/filter oracles and selection regressions.
use super::*;
#[test]
fn explicit_gpu_selection_never_uses_default_device() -> Result<()> {
    const CASE: &str = "TILEINK_GPU_SELECTION_CASE";
    if let Ok(case) = std::env::var(CASE) {
        let error = Routes::new()
            .err()
            .ok_or("coarse test accepted an absent or unavailable GPU identity")?;
        let expected = if case == "missing" {
            "environment variable not found"
        } else {
            "requested Metal registry ID unavailable"
        };
        assert!(error.to_string().contains(expected), "{error}");
        return Ok(());
    }
    let name = concat!(
        module_path!(),
        "::explicit_gpu_selection_never_uses_default_device"
    );
    let name = name.split_once("::").unwrap().1;
    for (case, identity) in [
        ("missing", None),
        ("empty", Some("")),
        ("unavailable", Some("ffffffffffffffff")),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .args(["--exact", name, "--test-threads=1"])
            .env(CASE, case)
            .env("MTL_DEBUG_LAYER", "1");
        if let Some(identity) = identity {
            command.env("TILEINK_NATIVE_GPU", identity);
        } else {
            command.env_remove("TILEINK_NATIVE_GPU");
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    }
    Ok(())
}

pub(super) struct Routes(std::cell::RefCell<Metal>);
impl Routes {
    pub(super) fn new() -> Result<Self> {
        Ok(Self(std::cell::RefCell::new(Metal::with_options(
            &gpu_support::options()?,
        )?)))
    }
    pub(super) fn check(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
    ) -> Result<()> {
        let mut device = self.0.borrow_mut();
        let ticket = device.submit_compute(batch)?;
        let actual = device.readback_batch(&ticket)?;
        assert_eq!(actual.len(), expected.len(), "{case}");
        for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
            let diff = a.iter().zip(b).position(|(a, b)| a != b);
            assert!(
                a.len() == b.len() && diff.is_none(),
                "{case}: buffer {i}, first different byte {diff:?}"
            );
        }
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.0.borrow().assert_valid()
    }
}
