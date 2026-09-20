//! The same retained/history and persistent-resource contracts run on Metal.
//! Imported Metal objects and event synchronization have additional tests in
//! metal/lifecycle_tests.rs; these cases exercise the shared renderer scheduler.
fn backend() -> crate::NativeBackend {
    crate::NativeBackend::Metal
}
#[path = "tests/completion_and_history_gpu.rs"]
mod completion_and_history;
#[path = "tests/persistent_buffer_gpu.rs"]
mod persistent_buffer;
#[path = "tests/persistent_image_gpu.rs"]
mod persistent_image;
#[path = "tests/persistent_texture_gpu.rs"]
mod persistent_texture;
