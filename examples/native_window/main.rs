//! Host-owned swapchains and native Tileink rendering; no CPU pixel round trip.
#[cfg(any(
    all(target_os = "windows", any(feature = "dx12", feature = "vulkan")),
    all(target_os = "macos", feature = "metal")
))]
#[path = "app.rs"]
mod app;
#[cfg(all(target_os = "windows", feature = "dx12"))]
#[path = "dx12.rs"]
mod dx12;
#[cfg(all(target_os = "windows", feature = "vulkan"))]
#[path = "vulkan.rs"]
mod vulkan;
#[cfg(any(
    all(target_os = "windows", any(feature = "dx12", feature = "vulkan")),
    all(target_os = "macos", feature = "metal")
))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
#[cfg(not(any(
    all(target_os = "windows", any(feature = "dx12", feature = "vulkan")),
    all(target_os = "macos", feature = "metal")
)))]
fn main() {
    eprintln!(
        "This example requires --no-default-features --features metal on macOS, or dx12/vulkan on Windows."
    );
}

#[cfg(all(target_os = "macos", feature = "metal"))]
mod metal;
