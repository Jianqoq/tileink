//! Shared gate for process-wide debug enablement and device creation.
use std::sync::{Mutex, MutexGuard};

pub(super) struct Creation {
    pub(super) device_created: bool,
    pub(super) debug_enabled: bool,
}

pub(super) fn lock(state: &Mutex<Creation>) -> Result<MutexGuard<'_, Creation>, &'static str> {
    // A panic can interrupt native creation before the state is committed.
    // Preserve poison and stop subsequent native calls instead of guessing state.
    state.lock().map_err(|_| "DX12 creation state was poisoned")
}
