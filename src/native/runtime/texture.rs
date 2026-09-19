use std::cell::Cell;
#[cfg(feature = "native-vulkan")]
use std::rc::Rc;

pub(crate) enum Allocation {
    #[cfg(feature = "native-dx12")]
    Dx12(windows::Win32::Graphics::Direct3D12::ID3D12Resource),
    #[cfg(feature = "native-vulkan")]
    Vulkan(Rc<super::vulkan::compute_texture::Image>),
}

pub(crate) struct State {
    pub allocation: Allocation,
    /// Published only after the queue accepts the initializing commands.
    pub initialized: Cell<bool>,
}
