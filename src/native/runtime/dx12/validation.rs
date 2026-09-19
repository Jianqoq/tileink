use super::Result;
use std::{cell::RefCell, collections::BTreeSet};
use windows::{
    Win32::{
        Foundation::{
            D3D12_ERROR_ADAPTER_NOT_FOUND, D3D12_ERROR_DRIVER_VERSION_MISMATCH, E_INVALIDARG,
        },
        Graphics::Direct3D12::*,
    },
    core::HRESULT,
};

#[derive(Clone)]
pub struct Validation {
    pub queue: Option<ID3D12InfoQueue>,
    rejected_cache_messages: RefCell<BTreeSet<u64>>,
}

impl Validation {
    pub fn new(queue: Option<ID3D12InfoQueue>) -> Self {
        Self {
            queue,
            rejected_cache_messages: RefCell::new(BTreeSet::new()),
        }
    }

    pub fn count(&self) -> u64 {
        unsafe {
            self.queue
                .as_ref()
                .map_or(0, |queue| queue.GetNumStoredMessages())
        }
    }
    pub fn record_cache_rejection(&self, start: u64) -> Result<()> {
        let Some(queue) = &self.queue else {
            return Ok(());
        };
        unsafe {
            for index in start..queue.GetNumStoredMessages() {
                let (id, _, description) = message(queue, index)?;
                if cache_message(id) {
                    // Keep the original queue intact and exempt only known cache
                    // diagnostics from this failed creation attempt. All unrelated
                    // errors, including earlier errors, still fail validation.
                    self.rejected_cache_messages.borrow_mut().insert(index);
                    eprintln!("native DX12 rejected pipeline cache: {description}");
                }
            }
        }
        Ok(())
    }
}

pub fn cache_retryable(code: HRESULT) -> bool {
    matches!(
        code,
        E_INVALIDARG | D3D12_ERROR_ADAPTER_NOT_FOUND | D3D12_ERROR_DRIVER_VERSION_MISMATCH
    )
}

fn cache_message(id: D3D12_MESSAGE_ID) -> bool {
    matches!(
        id,
        D3D12_MESSAGE_ID_CREATEPIPELINESTATE_INVALIDCACHEDBLOB
            | D3D12_MESSAGE_ID_CREATEPIPELINESTATE_CACHEDBLOBADAPTERMISMATCH
            | D3D12_MESSAGE_ID_CREATEPIPELINESTATE_CACHEDBLOBDRIVERVERSIONMISMATCH
            | D3D12_MESSAGE_ID_CREATEPIPELINESTATE_CACHEDBLOBDESCMISMATCH
    )
}

fn message(
    queue: &ID3D12InfoQueue,
    index: u64,
) -> Result<(D3D12_MESSAGE_ID, D3D12_MESSAGE_SEVERITY, String)> {
    unsafe {
        let mut size = 0;
        queue.GetMessage(index, None, &mut size)?;
        let mut storage = vec![0u64; size.div_ceil(8)];
        let pointer = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        queue.GetMessage(index, Some(pointer), &mut size)?;
        let message = &*pointer;
        let description = std::slice::from_raw_parts(
            message.pDescription.cast::<u8>(),
            message.DescriptionByteLength,
        );
        Ok((
            message.ID,
            message.Severity,
            String::from_utf8_lossy(description).into_owned(),
        ))
    }
}

pub fn assert_valid(validation: &Validation) -> Result<()> {
    validate(validation, false)
}

// Public contexts and whole-renderer parity tests opt in. wgpu's RTV initialization can emit
// this optimization advisory through the shared DX12 device info queue. Keep
// every message and all correctness warnings/errors; native validation is strict.
pub fn assert_valid_with_wgpu_clears(validation: &Validation) -> Result<()> {
    validate(validation, true)
}

fn validate(validation: &Validation, wgpu_clears: bool) -> Result<()> {
    let Some(queue) = &validation.queue else {
        return Ok(());
    };
    unsafe {
        for index in 0..queue.GetNumStoredMessages() {
            let (id, severity, description) = message(queue, index)?;
            if wgpu_clears
                && id == D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE
                && severity == D3D12_MESSAGE_SEVERITY_WARNING
            {
                eprintln!("wgpu DX12 optimized-clear advisory: {description}");
                continue;
            }
            if severity.0 <= D3D12_MESSAGE_SEVERITY_WARNING.0
                && !validation.rejected_cache_messages.borrow().contains(&index)
            {
                return Err(format!("DX12 validation: {description}").into());
            }
        }
    }
    Ok(())
}

#[test]
fn cache_recovery_never_masks_device_loss_or_unrelated_validation() {
    assert!(cache_retryable(E_INVALIDARG));
    assert!(cache_retryable(D3D12_ERROR_DRIVER_VERSION_MISMATCH));
    assert!(!cache_retryable(
        windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED
    ));
    assert!(!cache_retryable(windows::Win32::Foundation::E_OUTOFMEMORY));
    assert!(cache_message(
        D3D12_MESSAGE_ID_CREATEPIPELINESTATE_CACHEDBLOBDESCMISMATCH
    ));
    assert!(!cache_message(D3D12_MESSAGE_ID_UNKNOWN));
}
