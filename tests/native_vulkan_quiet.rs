//! Successful cold and warm pipeline creation must not pollute application stderr.
#![cfg(feature = "vulkan")]

use std::process::Command;

use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, NativeBackend, NativeRenderer, Radius};

#[test]
#[ignore = "requires a native GPU"]
fn successful_pipeline_creation_is_quiet() -> Result<(), Box<dyn std::error::Error>> {
    const CHILD: &str = "TILEINK_QUIET_PIPELINE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for _ in 0..2 {
            let mut renderer = NativeRenderer::new(NativeBackend::Vulkan, 24, 24)?;
            let mut canvas = Canvas::new(24, 24, 1.0);
            canvas.push_rect(
                Rect::new(2.0, 2.0, 22.0, 22.0),
                Radius::all(0.0),
                Color::BLACK,
            );
            let image = renderer.render_to_image(&canvas)?.readback()?;
            assert_eq!(image.pixels[12 * 24 + 12] >> 24, 255);
        }
        return Ok(());
    }
    let directory = tempfile::tempdir()?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "successful_pipeline_creation_is_quiet",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env("TILEINK_NATIVE_PIPELINE_CACHE_DIR", directory.path())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        !stdout.contains("native Vulkan compute ") && !stderr.contains("native Vulkan compute "),
        "successful Vulkan pipeline creation wrote diagnostic output: {stdout}\n{stderr}"
    );
    Ok(())
}
