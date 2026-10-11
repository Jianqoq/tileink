//! Exercise the actual creation gate without loading D3D12 or creating a GPU.
#[path = "../src/native/runtime/dx12/creation.rs"]
mod creation;
use creation::Creation;
use std::sync::{Mutex, TryLockError};

#[test]
fn healthy_creation_gate_preserves_state_and_exclusive_ownership() {
    let state = Mutex::new(Creation {
        device_created: false,
        debug_enabled: true,
    });
    {
        let mut guard = creation::lock(&state).unwrap();
        assert!(!guard.device_created && guard.debug_enabled);
        assert!(matches!(state.try_lock(), Err(TryLockError::WouldBlock)));
        guard.device_created = true;
    }
    let guard = creation::lock(&state).unwrap();
    assert!(guard.device_created && guard.debug_enabled);
}

#[test]
fn poisoned_native_creation_gate_rejects_reentry() {
    let state = Mutex::new(Creation {
        device_created: false,
        debug_enabled: false,
    });
    assert!(
        std::panic::catch_unwind(|| {
            let mut guard = creation::lock(&state).unwrap();
            guard.device_created = true;
            panic!("native creation interrupted before state was committed");
        })
        .is_err()
    );
    assert!(state.is_poisoned());
    for _ in 0..2 {
        assert_eq!(
            creation::lock(&state).err(),
            Some("DX12 creation state was poisoned")
        );
        assert!(state.is_poisoned());
    }
}
