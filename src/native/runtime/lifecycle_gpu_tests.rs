//! Native lifetime checks run in each legal single-backend build.
#[cfg(feature = "dx12")]
use super::dx12::Dx12 as Device;
use super::program::{Params, Probe};
#[cfg(feature = "vulkan")]
use super::vulkan::Vulkan as Device;
use super::{Result, submissions};

#[path = "tests/batches.rs"]
mod batches;
#[path = "tests/cases.rs"]
mod cases;
#[path = "tests/interop_gpu.rs"]
mod interop;
#[path = "tests/persistent_buffer_gpu.rs"]
mod persistent_buffer;
#[path = "tests/persistent_image_gpu.rs"]
mod persistent_image;
#[path = "tests/persistent_texture_gpu.rs"]
mod persistent_texture;
#[cfg(feature = "vulkan")]
#[path = "tests/vulkan_target_use_gpu.rs"]
mod vulkan_target_use;

fn backend() -> crate::NativeBackend {
    #[cfg(feature = "dx12")]
    {
        crate::NativeBackend::Dx12
    }
    #[cfg(feature = "vulkan")]
    {
        crate::NativeBackend::Vulkan
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn queued_native_submissions_keep_leases_and_reject_wrong_devices() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let mut device = Device::new(&identity)?;
    let mut other = Device::new(&identity)?;
    #[cfg(feature = "dx12")]
    let messages = device.validation_queue();
    #[cfg(feature = "vulkan")]
    let messages = device.validation_messages();
    let cases = cases::cases();
    let tickets = cases
        .iter()
        .map(|case| device.submit(case))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(device.pending_count(), cases.len());
    assert_eq!(
        other
            .readback(&tickets[0])
            .unwrap_err()
            .downcast_ref::<submissions::SubmissionError>(),
        Some(&submissions::SubmissionError::WrongDevice)
    );
    // Newest-first readback cannot overwrite or discard earlier unread outputs.
    for (case, ticket) in cases.iter().zip(&tickets).rev() {
        assert_eq!(device.readback(ticket)?, case.expected);
    }
    assert_eq!(device.pending_count(), 0);
    assert!(device.readback(&tickets[0]).is_err());
    // Dropping a receipt does not retire GPU work; teardown must still cover it.
    drop(device.submit(&cases[0])?);
    assert_eq!(device.pending_count(), 1);
    drop(device);
    drop(other);
    #[cfg(feature = "dx12")]
    super::dx12::assert_valid(&messages)?;
    #[cfg(feature = "vulkan")]
    assert!(messages.lock().unwrap().is_empty());
    Ok(())
}

#[path = "tests/completion_and_history_gpu.rs"]
mod completion_and_history;
