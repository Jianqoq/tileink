use super::*;
use crate::native::interop::dx12::TargetSynchronization;
pub fn validate(
    device: &ID3D12Device,
    target: &ID3D12Resource,
    sync: &TargetSynchronization,
) -> Result<()> {
    let descriptor = unsafe { target.GetDesc() };
    validate_state(&descriptor, sync.incoming)?;
    validate_state(&descriptor, sync.outgoing)?;
    for point in sync.waits.iter().chain(&sync.signals) {
        unsafe {
            let mut owner = None;
            point.fence.GetDevice(&mut owner)?;
            let owner: ID3D12Device = owner.ok_or("external fence has no device")?;
            if owner.cast::<windows::core::IUnknown>()?.as_raw()
                != device.cast::<windows::core::IUnknown>()?.as_raw()
            {
                return Err("external fence belongs to another DX12 device".into());
            }
            if point.value == u64::MAX {
                return Err("UINT64_MAX is reserved for device removal".into());
            }
        }
    }
    Ok(())
}

pub fn validate_state(
    descriptor: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
) -> Result<()> {
    let shader =
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    let allowed = shader
        | D3D12_RESOURCE_STATE_COPY_SOURCE
        | D3D12_RESOURCE_STATE_COPY_DEST
        | D3D12_RESOURCE_STATE_RENDER_TARGET
        | D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    if state.0 & !allowed.0 != 0 {
        return Err("unsupported DX12 image state".into());
    }
    for write in [
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
    ] {
        if state.0 & write.0 != 0 && state != write {
            return Err("DX12 write states must be exclusive".into());
        }
    }
    if (state.0 & shader.0 != 0
        && descriptor
            .Flags
            .contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE))
        || (state == D3D12_RESOURCE_STATE_RENDER_TARGET
            && !descriptor
                .Flags
                .contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET))
        || (state == D3D12_RESOURCE_STATE_UNORDERED_ACCESS
            && !descriptor
                .Flags
                .contains(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS))
    {
        return Err("DX12 target state is incompatible with allocation flags".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_write_combinations_and_unavailable_target_usage() {
        let descriptor = D3D12_RESOURCE_DESC {
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        assert!(
            validate_state(
                &descriptor,
                D3D12_RESOURCE_STATE_COPY_SOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
            )
            .is_ok()
        );
        for state in [
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS | D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
        ] {
            assert!(validate_state(&descriptor, state).is_err());
        }
    }
}
