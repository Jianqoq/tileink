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
    unsafe {
        let mut debug = None;
        D3D12GetDebugInterface(&mut debug)?;
        let debug: ID3D12Debug = debug.unwrap();
        debug.EnableDebugLayer();
    }
    state.debug_enabled = true;
    Ok(())
}

pub(super) fn create_device(adapter: &IDXGIAdapter1) -> Result<ID3D12Device> {
    let mut state = creation::lock(&CREATION)?;
    let mut device = None;
    unsafe {
        D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device)?;
    }
    state.device_created = true;
    Ok(device.unwrap())
}
