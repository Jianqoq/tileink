use std::cell::Cell;
#[cfg(any(feature = "dx12", feature = "vulkan"))]
use std::rc::Rc;

pub(crate) enum Allocation {
    #[cfg(feature = "metal")]
    Metal(objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>>),
    #[cfg(feature = "dx12")]
    Dx12(Rc<Dx12Allocation>),
    #[cfg(feature = "vulkan")]
    Vulkan(Rc<super::vulkan::compute_texture::Image>),
}

pub(crate) struct State {
    pub allocation: Allocation,
    /// Published only after the queue accepts the initializing commands.
    pub initialized: Cell<bool>,
    pub content_version: Cell<u64>,
}

#[cfg(feature = "dx12")]
pub(crate) struct Dx12Allocation {
    pub resource: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub state: Cell<windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES>,
    pub final_state: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
}
