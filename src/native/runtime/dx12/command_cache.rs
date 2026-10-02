use super::*;

/// Only explicitly completed frames enter the free list. Keeping both objects
/// avoids per-frame driver allocation; an allocator must never reset in flight.
#[derive(Clone)]
pub(super) struct Commands {
    pub list: ID3D12GraphicsCommandList,
    pub allocator: ID3D12CommandAllocator,
}

impl Commands {
    pub(super) fn acquire(device: &ID3D12Device, free: &mut Vec<Self>) -> Result<Self> {
        unsafe {
            if let Some(commands) = free.pop() {
                commands.allocator.Reset()?;
                commands.list.Reset(&commands.allocator, None)?;
                return Ok(commands);
            }
            let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            Ok(Self { list, allocator })
        }
    }
}
