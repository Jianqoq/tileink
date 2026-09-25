use std::time::Duration;

pub(crate) mod cpu;

/// Adapter-neutral timing data; absent GPU duration preserves an unsupported scope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderProfileEntry {
    pub name: &'static str,
    pub cpu_duration: Option<Duration>,
    pub gpu_duration: Option<Duration>,
}
