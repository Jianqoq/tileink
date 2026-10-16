//! Debug-layer enablement is process-wide, not a per-device switch.
use super::creation::{self, Creation};
use super::*;
use std::sync::Mutex;

static CREATION: Mutex<Creation> = Mutex::new(Creation {
    device_created: false,
    debug_enabled: false,
});

/// # Safety
/// Before the first successful call, no externally created DX12 device may exist
/// and no external device creation may run concurrently. Tileink-owned creation is
/// serialized here; late requests are rejected without touching the debug layer.
pub(crate) unsafe fn enable_validation() -> Result<()> {
    let mut state = creation::lock(&CREATION)?;
    if state.debug_enabled {
        return Ok(());
    }
    if state.device_created {
        return Err("DX12 debug layer must be enabled before creating a device".into());
    }
    // SAFETY: The creation mutex excludes concurrent device creation; no device has been created before enabling the debug layer.
    unsafe {
        let mut debug = None;
        D3D12GetDebugInterface(&raw mut debug)?;
        let debug: ID3D12Debug = debug.ok_or("DX12 returned no debug interface")?;
        debug.EnableDebugLayer();
    }
    state.debug_enabled = true;
    drop(state);
    Ok(())
}

pub(super) fn create_device(adapter: &IDXGIAdapter1) -> Result<ID3D12Device> {
    let mut state = creation::lock(&CREATION)?;
    let mut device = None;
    // SAFETY: The creation mutex serializes device creation with debug setup; adapter and output storage remain live during the call.
    unsafe {
        D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &raw mut device)?;
    }
    state.device_created = true;
    drop(state);
    device.ok_or_else(|| "DX12 returned no created device".into())
}
