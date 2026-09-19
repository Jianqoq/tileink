//! Host-owned swapchains and native Tileink rendering; no CPU pixel round trip.
#[cfg(all(target_os = "windows", any(feature = "dx12", feature = "vulkan")))]
#[path = "app.rs"]
mod app;
#[cfg(all(target_os = "windows", feature = "dx12"))]
#[path = "dx12.rs"]
mod dx12;
#[cfg(all(target_os = "windows", feature = "vulkan"))]
#[path = "vulkan.rs"]
mod vulkan;
#[cfg(all(target_os = "windows", any(feature = "dx12", feature = "vulkan")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
#[cfg(not(all(target_os = "windows", any(feature = "dx12", feature = "vulkan"))))]
fn main() {
    eprintln!(
        "This example requires Windows and --no-default-features --features dx12 (or vulkan)."
    );
}
