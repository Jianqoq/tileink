//! Host-owned swapchains and native Tileink rendering; no CPU pixel round trip.
#[cfg(target_os = "windows")]
#[path = "app.rs"]
mod app;
#[cfg(target_os = "windows")]
#[path = "dx12.rs"]
mod dx12;
#[cfg(target_os = "windows")]
#[path = "vulkan.rs"]
mod vulkan;
#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This window-host example currently requires Windows.");
}
