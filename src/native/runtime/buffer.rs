//! Persistent device-local buffer ownership; frames retain allocations, not devices.
use super::{Result, adapter::Adapter};
use std::{cell::Cell, rc::Rc};

pub enum Allocation {
    #[cfg(feature = "dx12")]
    Dx12(windows::Win32::Graphics::Direct3D12::ID3D12Resource),
    #[cfg(feature = "vulkan")]
    Vulkan(Rc<super::vulkan::compute_memory::Arena>),
}
pub struct State {
    pub allocation: Allocation,
    pub size: usize,
    pub initialized: Cell<bool>,
}
#[derive(Clone)]
pub struct Buffer {
    pub state: Rc<State>,
    pub adapter: Adapter,
}
impl Buffer {
    pub fn new(adapter: &Adapter, size: usize) -> Result<Self> {
        if size == 0 || !size.is_multiple_of(4) || size > u32::MAX as usize {
            return Err("invalid persistent native buffer size".into());
        }
        Ok(Self {
            state: Rc::new(State {
                allocation: adapter.allocate_buffer(size)?,
                size,
                initialized: Cell::new(false),
            }),
            adapter: adapter.clone(),
        })
    }
}
