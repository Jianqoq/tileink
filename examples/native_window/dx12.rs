use super::app::Result;
use super::platform::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tileink::native_interop::dx12::TargetSynchronization;
use tileink::{
    NativeContext, NativeSubmission, NativeTexture,
    native_interop::dx12::{ContextDescriptor, TextureDescriptor},
};
use tileink::{NativeRenderTarget, NativeTargetSubmission, NativeTargetUse};
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_12_0,
            Direct3D12::*,
            Dxgi::{Common::*, *},
        },
    },
    core::Interface,
};
#[path = "dx12_copy.rs"]
mod copy;
struct Frame {
    copy: copy::Commands,
    completion: u64,
    rendering: Option<NativeSubmission>,
}
pub struct Host {
    image: Option<NativeTexture>,
    source: Option<ID3D12Resource>,
    frames: Vec<Frame>,
    context: NativeContext,
    swapchain: IDXGISwapChain3,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    serial: u64,
    size: [u32; 2],
    index: usize,
    unconfirmed: bool,
    incoming: D3D12_RESOURCE_STATES,
}
impl Host {
    pub fn new(window: &Window) -> Result<Self> {
        unsafe {
            NativeContext::enable_dx12_validation()?;
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let adapter = factory.EnumAdapters1(0)?;
            let mut device = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device)?;
            let device: ID3D12Device = device.ok_or("missing DX12 device")?;
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    ..Default::default()
                })?;
            let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
                return Err("expected Win32 window".into());
            };
            let size = window.inner_size();
            let swapchain: IDXGISwapChain3 = factory
                .CreateSwapChainForHwnd(
                    &queue,
                    HWND(handle.hwnd.get() as *mut _),
                    &DXGI_SWAP_CHAIN_DESC1 {
                        Width: size.width,
                        Height: size.height,
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                        BufferCount: 2,
                        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                        ..Default::default()
                    },
                    None,
                    None,
                )?
                .cast()?;
            let context = NativeContext::from_dx12(ContextDescriptor {
                device: device.clone(),
                queue: queue.clone(),
                validation: true,
            })?;
            let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let mut frames = Vec::new();
            for _ in 0..2 {
                frames.push(Frame {
                    copy: copy::Commands::new(&device)?,
                    completion: 0,
                    rendering: None,
                });
            }
            let mut this = Self {
                image: None,
                source: None,
                frames,
                context,
                swapchain,
                device,
                queue,
                fence,
                serial: 0,
                size: [size.width, size.height],
                index: 0,
                unconfirmed: false,
                incoming: D3D12_RESOURCE_STATE_COMMON,
            };
            this.create_target()?;
            Ok(this)
        }
    }
    fn create_target(&mut self) -> Result {
        self.incoming = D3D12_RESOURCE_STATE_COMMON;
        // DXGI flip buffers cannot be UAV targets on this host. Render directly
        // into a persistent host RGBA8 allocation, then perform one GPU-only copy.
        unsafe {
            let mut resource = None;
            self.device.CreateCommittedResource(
                &D3D12_HEAP_PROPERTIES {
                    Type: D3D12_HEAP_TYPE_DEFAULT,
                    ..Default::default()
                },
                D3D12_HEAP_FLAG_NONE,
                &D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                    Width: self.size[0] as u64,
                    Height: self.size[1],
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                    ..Default::default()
                },
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )?;
            let resource: ID3D12Resource = resource.ok_or("missing host render target")?;
            self.image = Some(self.context.import_dx12_texture(TextureDescriptor {
                resource: resource.clone(),
                initialized: false,
                initial_state: D3D12_RESOURCE_STATE_COMMON,
                final_state: D3D12_RESOURCE_STATE_COPY_SOURCE,
            })?);
            self.source = Some(resource);
        }
        Ok(())
    }
    fn wait(&mut self, index: usize) -> Result {
        let frame = &mut self.frames[index];
        unsafe {
            if self.fence.GetCompletedValue() == u64::MAX {
                return Err("DX12 device removed".into());
            }
            if self.fence.GetCompletedValue() < frame.completion {
                // A null event makes this explicit frame-retirement call block.
                self.fence
                    .SetEventOnCompletion(frame.completion, HANDLE::default())?;
            }
        }
        if let Some(rendering) = frame.rendering.take() {
            rendering.wait()?;
        }
        Ok(())
    }
    fn retire(&mut self) -> Result {
        for index in 0..self.frames.len() {
            self.wait(index)?;
        }
        Ok(())
    }
}
impl super::app::Host for Host {
    fn preserves_target(&self) -> bool {
        true
    }
    fn context(&self) -> &NativeContext {
        &self.context
    }
    fn acquire(&mut self, size: [u32; 2]) -> Result<NativeTexture> {
        if self.size != size {
            self.retire()?;
            self.image = None;
            self.source = None;
            // Reset completed command lists to release their back-buffer references.
            for frame in &mut self.frames {
                frame.copy = copy::Commands::new(&self.device)?;
            }
            unsafe {
                self.swapchain.ResizeBuffers(
                    2,
                    size[0],
                    size[1],
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
            }
            self.size = size;
            self.create_target()?;
        }
        self.index = unsafe { self.swapchain.GetCurrentBackBufferIndex() } as usize;
        self.wait(self.index)?;
        Ok(self.image.as_ref().unwrap().clone())
    }
    fn target_use<'a>(&self, target: NativeRenderTarget<'a>) -> Result<NativeTargetUse<'a>> {
        Ok(unsafe {
            self.context.dx12_target_use(
                target,
                TargetSynchronization {
                    incoming: self.incoming,
                    outgoing: D3D12_RESOURCE_STATE_COPY_SOURCE,
                    waits: Vec::new(),
                    signals: Vec::new(),
                },
            )?
        })
    }
    fn present(&mut self, submission: NativeTargetSubmission) -> Result {
        let frame = &mut self.frames[self.index];
        if submission.outgoing.state != D3D12_RESOURCE_STATE_COPY_SOURCE {
            return Err("unexpected DX12 output state".into());
        }
        self.incoming = D3D12_RESOURCE_STATE_COPY_SOURCE;
        frame.rendering = Some(submission.submission);
        unsafe {
            let destination: ID3D12Resource = self.swapchain.GetBuffer(self.index as u32)?;
            self.unconfirmed = true;
            frame
                .copy
                .submit(&self.queue, self.source.as_ref().unwrap(), &destination)?;
            self.serial += 1;
            // Completion covers host copy commands, beyond Tileink's own fence.
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.queue.Signal(&self.fence, self.serial)?;
            frame.completion = self.serial;
            self.unconfirmed = false;
        }
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if self.unconfirmed || self.retire().is_err() {
            // A failed fence signal cannot prove host copy/present completion.
            // Keep the complete GPU ownership chain, just as Tileink does internally.
            std::mem::forget((
                self.device.clone(),
                self.queue.clone(),
                self.fence.clone(),
                self.swapchain.clone(),
                self.image.clone(),
                self.source.clone(),
                self.context.clone(),
                self.frames
                    .iter()
                    .map(|frame| frame.copy.clone())
                    .collect::<Vec<_>>(),
            ));
        }
    }
}
