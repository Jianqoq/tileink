//! Monotonic CPU timers for renderer profiling.
//!
//! Native builds use `std::time::Instant`. Wasm uses `Performance::now()` because
//! browser environments do not provide a dependable `std::time::Instant`.

use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CpuInstant {
    #[cfg(not(target_arch = "wasm32"))]
    native: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    millis: f64,
}

impl CpuInstant {
    #[inline]
    pub(crate) fn now() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                native: std::time::Instant::now(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                millis: performance_now(),
            }
        }
    }

    #[inline]
    pub(crate) fn elapsed(&self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native.elapsed()
        }
        #[cfg(target_arch = "wasm32")]
        {
            duration_from_millis((performance_now() - self.millis).max(0.0))
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

#[cfg(target_arch = "wasm32")]
fn duration_from_millis(millis: f64) -> Duration {
    if !millis.is_finite() || millis <= 0.0 {
        return Duration::ZERO;
    }
    let nanos = (millis * 1_000_000.0).round();
    if nanos >= u64::MAX as f64 {
        Duration::from_nanos(u64::MAX)
    } else {
        Duration::from_nanos(nanos as u64)
    }
}
