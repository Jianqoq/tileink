use crate::native::{NativeBackend, NativeContext, NativeError};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};

/// COM references are retained for the lifetime of the context and its submissions.
pub struct ContextDescriptor {
    pub device: ID3D12Device,
    pub queue: ID3D12CommandQueue,
    pub validation: bool,
}
impl NativeContext {
    /// Use an application's existing DIRECT queue and device.
    ///
    /// # Safety
    /// The host must serialize use of this queue with Tileink calls and preserve
    /// resource state and synchronization contracts for every imported image.
    /// Enable DX12 validation before creating the device when requested.
    pub unsafe fn from_dx12(descriptor: ContextDescriptor) -> Result<Self, NativeError> {
        let adapter = crate::native::runtime::adapter::Adapter::from_dx12(descriptor)
            .map_err(NativeError::Initialization)?;
        Ok(Self::from_adapter(NativeBackend::Dx12, adapter))
    }
}

/// A single-mip RGBA8 image with UAV and shader-read usage. Host writes between Tileink calls must
/// restore `final_state` for ordinary calls, or provide an explicit TargetUse.
pub struct TextureDescriptor {
    pub resource: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    /// False initializes undefined contents before the first use.
    pub initialized: bool,
    pub initial_state: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
    pub final_state: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
}
impl NativeContext {
    /// Import an image without taking over host state or swapchain policy.
    ///
    /// # Safety
    /// The resource must be in `initial_state` when
    /// the queue reaches Tileink. Host accesses must be GPU-synchronized with this
    /// queue. Ordinary calls start in the previous tracked state and restore
    /// `final_state`; an explicit TargetUse overrides both per-use states.
    /// Set `initialized` only when every pixel has defined contents; reimport after
    /// host writes to establish a new retained-content identity.
    pub unsafe fn import_dx12_texture(
        &self,
        descriptor: TextureDescriptor,
    ) -> Result<crate::NativeTexture, NativeError> {
        let initialized = descriptor.initialized;
        let (allocation, size) = self
            .adapter
            .import_dx12_texture(descriptor)
            .map_err(NativeError::Initialization)?;
        Ok(crate::NativeTexture {
            state: std::rc::Rc::new(crate::native::runtime::texture::State {
                allocation,
                initialized: std::cell::Cell::new(initialized),
                content_version: std::cell::Cell::new(0),
            }),
            context: self.clone(),
            size,
            layers: 1,
            array: false,
        })
    }
}

/// A typed GPU fence dependency; COM references remain pinned through completion.
#[derive(Clone)]
pub struct FencePoint {
    pub fence: windows::Win32::Graphics::Direct3D12::ID3D12Fence,
    pub value: u64,
}
/// One use of an allocation. The renderer consumes the enclosing TargetUse.
#[derive(Clone)]
pub struct TargetSynchronization {
    pub incoming: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
    pub outgoing: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
    pub waits: Vec<FencePoint>,
    pub signals: Vec<FencePoint>,
}
impl NativeContext {
    /// Describe incoming/outgoing state and GPU dependencies for one render use.
    /// The target's history ID must change after external content modifications.
    ///
    /// # Safety
    /// Incoming state must describe the resource when waits complete, and the host
    /// must preserve the returned outgoing state until the next synchronized use.
    pub unsafe fn dx12_target_use<'a>(
        &self,
        target: crate::NativeRenderTarget<'a>,
        synchronization: TargetSynchronization,
    ) -> Result<crate::NativeTargetUse<'a>, NativeError> {
        if self.backend() != NativeBackend::Dx12
            || !self.adapter.same_device(&target.texture.context.adapter)
        {
            return Err(NativeError::Initialization(
                "DX12 target use requires its registered context".into(),
            ));
        }
        Ok(crate::NativeTargetUse {
            target,
            synchronization: super::Synchronization::Dx12(synchronization),
        })
    }
}
