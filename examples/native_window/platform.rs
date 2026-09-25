//! Small native host for this rendering example; application frameworks own their
//! full input/lifecycle policy. No third-party window/event-loop implementation.
#[derive(Clone, Copy)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Window;
#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "windows")]
pub use win32::Window;
