//! Execute the production Vulkan cache path through a controlled ash function table.
//! This verifies driver-call/error/ownership semantics, not Vulkan GPU conformance.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[path = "native_vulkan_pipeline_cache/driver.rs"]
mod driver;
#[path = "../build/hex.rs"]
mod hex;
#[path = "../src/native/runtime/pipeline_cache.rs"]
mod pipeline_cache;
#[allow(
    dead_code,
    reason = "Shared compiler helpers expose different entry points in standalone tests and backend builds."
)] // This standalone driver-cache crate consumes hit status, not shader bytes.
#[path = "../build/native/cache.rs"]
mod shader_cache;
#[path = "../src/native/runtime/vulkan/compute_pipeline/cache.rs"]
mod vulkan_cache;

use ash::vk;
use std::{fs, path::Path, process::Command};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn worker(name: &str, run: impl FnOnce(&Path)) {
    if let Some(root) = std::env::var_os("TILEINK_VULKAN_CACHE_TEST_ROOT") {
        run(Path::new(&root));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1"])
        .env("TILEINK_VULKAN_CACHE_TEST_ROOT", root.path())
        .env("TILEINK_NATIVE_PIPELINE_CACHE_DIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn properties() -> vk::PhysicalDeviceProperties {
    vk::PhysicalDeviceProperties {
        vendor_id: 23,
        device_id: 41,
        pipeline_cache_uuid: [17; 16],
        ..Default::default()
    }
}
fn invoke(key: &str, properties: &vk::PhysicalDeviceProperties) -> Result<bool> {
    let device = driver::device();
    let (pipeline, hit) = vulkan_cache::create(
        &device,
        properties,
        key,
        vk::PipelineShaderStageCreateInfo::default(),
        vk::PipelineLayout::null(),
    )?;
    // SAFETY: This pipeline is owned by the fake device and was never submitted; destroy releases its test handle.
    unsafe {
        device.destroy_pipeline(pipeline, None);
    }
    Ok(hit)
}
fn create(key: &str) -> Result<bool> {
    invoke(key, &properties())
}
fn assert_error(result: Result<bool>, expected: vk::Result) {
    let error = result.unwrap_err();
    let io = error.downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(
        io.get_ref().unwrap().downcast_ref::<vk::Result>(),
        Some(&expected)
    );
    driver::with(|d| {
        assert!(d.caches.is_empty());
        assert!(d.pipelines.is_empty());
        assert!(!d.invalid_destroy);
    });
}
fn cached_error(operation: &'static str) {
    for (i, error) in [
        vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        vk::Result::ERROR_OUT_OF_DEVICE_MEMORY,
        vk::Result::ERROR_UNKNOWN,
    ]
    .into_iter()
    .enumerate()
    {
        let key = format!("{operation}-{i}");
        assert!(!create(&key).unwrap());
        driver::with(|d| {
            d.reset_calls();
            d.fail = Some((operation, error));
        });
        assert_error(create(&key), error);
        driver::with(|d| {
            assert_eq!(
                d.calls.iter().filter(|&&c| c == "cache").count(),
                1,
                "driver failure must not trigger a cold retry"
            );
        });
        assert!(
            create(&key).unwrap(),
            "failed cache acceptance must preserve the existing entry"
        );
    }
}

#[test]
fn cached_cache_creation_errors_propagate_without_retry() {
    worker(
        "cached_cache_creation_errors_propagate_without_retry",
        |_| cached_error("cache"),
    );
}
#[test]
fn cached_pipeline_creation_errors_propagate_without_retry() {
    worker(
        "cached_pipeline_creation_errors_propagate_without_retry",
        |_| cached_error("pipeline"),
    );
}
#[test]
fn cache_hits_do_not_export_unchanged_driver_data() {
    worker("cache_hits_do_not_export_unchanged_driver_data", |_| {
        assert!(!create("hit").unwrap());
        driver::with(|d| {
            d.reset_calls();
            d.fail = Some(("data", vk::Result::ERROR_OUT_OF_HOST_MEMORY));
        });
        assert!(create("hit").unwrap());
        driver::with(|d| {
            assert_eq!(d.calls, ["cache", "pipeline"]);
            assert!(d.caches.is_empty() && d.pipelines.is_empty());
        });
    });
}
#[test]
fn opaque_driver_cache_is_forwarded_without_host_header_policy() {
    worker(
        "opaque_driver_cache_is_forwarded_without_host_header_policy",
        |_| {
            driver::with(|d| d.bytes[16..32].fill(99));
            assert!(!create("opaque").unwrap());
            driver::with(driver::Driver::reset_calls);
            assert!(create("opaque").unwrap());
            driver::with(|d| assert_eq!(d.inputs.as_slice(), std::slice::from_ref(&d.bytes)));
        },
    );
}
#[test]
fn cold_driver_errors_retire_cache_and_partial_pipeline_without_publication() {
    worker(
        "cold_driver_errors_retire_cache_and_partial_pipeline_without_publication",
        |root| {
            for (i, operation) in ["cache", "pipeline", "data"].into_iter().enumerate() {
                let key = format!("cold-{i}");
                driver::with(|d| {
                    d.reset_calls();
                    d.fail = Some((operation, vk::Result::ERROR_OUT_OF_DEVICE_MEMORY));
                    d.partial = operation == "pipeline";
                });
                assert_error(create(&key), vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
                assert_eq!(
                    fs::read_dir(root)
                        .unwrap()
                        .filter(|p| p
                            .as_ref()
                            .unwrap()
                            .path()
                            .extension()
                            .is_some_and(|e| e == "shader"))
                        .count(),
                    i
                );
                driver::with(|d| d.partial = false);
                assert!(!create(&key).unwrap());
            }
        },
    );
}
#[test]
fn failed_cache_publication_retires_created_native_objects() {
    worker(
        "failed_cache_publication_retires_created_native_objects",
        |root| {
            driver::with(|d| d.block_publication = Some(root.into()));
            assert!(
                create("publication")
                    .unwrap_err()
                    .downcast_ref::<std::io::Error>()
                    .is_some()
            );
            driver::with(|d| {
                assert!(d.caches.is_empty() && d.pipelines.is_empty());
                assert!(!d.invalid_destroy);
            });
        },
    );
}
#[test]
fn driver_cache_identity_separates_shader_and_device_versions() {
    worker(
        "driver_cache_identity_separates_shader_and_device_versions",
        |_| {
            let initial = properties();
            assert!(!invoke("identity", &initial).unwrap());
            assert!(invoke("identity", &initial).unwrap());
            assert!(!invoke("another-shader", &initial).unwrap());
            for field in 0..5 {
                let mut changed = initial;
                match field {
                    0 => changed.vendor_id += 1,
                    1 => changed.device_id += 1,
                    2 => changed.driver_version += 1,
                    3 => changed.api_version += 1,
                    _ => changed.pipeline_cache_uuid[0] += 1,
                }
                assert!(!invoke("identity", &changed).unwrap());
                assert!(invoke("identity", &changed).unwrap());
            }
            driver::with(|d| assert!(d.caches.is_empty() && d.pipelines.is_empty()));
        },
    );
}
