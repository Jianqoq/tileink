//! The host owns the NSView/CAMetalLayer and two queues. A shared event hands the
//! RGBA target to Tileink, then to a host render pass converting into a BGRA drawable.
//! Presentation never reads pixels back or waits on the CPU; teardown is explicit.
mod present;
use super::app::Result;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGSize;
use objc2_metal::*;
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use std::collections::VecDeque;
use tileink::native_interop::metal::{
    ContextDescriptor, EventPoint, TargetSynchronization, TextureDescriptor,
};
use tileink::{
    NativeContext, NativeRenderTarget, NativeSubmission, NativeTargetSubmission, NativeTargetUse,
    NativeTexture,
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};
type Object<T> = Retained<ProtocolObject<T>>;
struct Frame {
    command: Object<dyn MTLCommandBuffer>,
    rendering: NativeSubmission,
}
impl Frame {
    fn complete(&self) -> Result<bool> {
        match self.command.status() {
            MTLCommandBufferStatus::Completed => Ok(self.rendering.is_complete()?),
            MTLCommandBufferStatus::Error => {
                Err(format!("Metal presentation failed: {:?}", self.command.error()).into())
            }
            _ => Ok(false),
        }
    }
}
pub struct Host {
    context: NativeContext,
    device: Object<dyn MTLDevice>,
    queue: Object<dyn MTLCommandQueue>,
    layer: Retained<CAMetalLayer>,
    pipeline: Object<dyn MTLRenderPipelineState>,
    event: Object<dyn MTLSharedEvent>,
    serial: u64,
    target: Option<(NativeTexture, Object<dyn MTLTexture>)>,
    drawable: Option<Object<dyn CAMetalDrawable>>,
    frames: VecDeque<Frame>,
}
impl Host {
    pub fn new(window: &Window) -> Result<Self> {
        let device = if let Ok(id) = std::env::var("TILEINK_NATIVE_GPU") {
            MTLCopyAllDevices()
                .iter()
                .find(|device| format!("{:016x}", device.registryID()) == id)
                .ok_or("requested Metal GPU unavailable")?
        } else {
            MTLCreateSystemDefaultDevice().ok_or("Metal device unavailable")?
        };
        let queue = device.newCommandQueue().ok_or("host queue")?;
        let renderer_queue = device.newCommandQueue().ok_or("Tileink queue")?;
        // SAFETY: both queues and all accesses are owned here. Per-use shared
        // event values serialize reads and writes across the two queues.
        let context = unsafe {
            NativeContext::from_metal(ContextDescriptor {
                device: device.clone(),
                queue: renderer_queue,
            })?
        };
        let event = device.newSharedEvent().ok_or("shared event")?;
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(&device));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        layer.setFramebufferOnly(true);
        layer.setAllowsNextDrawableTimeout(true);
        let RawWindowHandle::AppKit(handle) = window.window_handle()?.as_raw() else {
            return Err("expected AppKit window".into());
        };
        // SAFETY: Winit owns a live NSView; ApplicationHandler runs on the main
        // thread. The window outlives Host, and NSView retains the assigned layer.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        view.setWantsLayer(true);
        view.setLayer(Some(&layer));
        let pipeline = present::pipeline(&device)?;
        println!(
            "Metal host GPU: {} ({:016x}); separate rendering/presentation queues",
            device.name(),
            device.registryID()
        );
        Ok(Self {
            context,
            device,
            queue,
            layer,
            pipeline,
            event,
            serial: 0,
            target: None,
            drawable: None,
            frames: VecDeque::new(),
        })
    }
    fn retire(&mut self) -> Result {
        while let Some(frame) = self.frames.front() {
            // Event signals can precede command-buffer completion notification.
            // Retire only after both receipts are observed complete, so this
            // ordinary frame path never waits for a rendering queue notification.
            if !frame.complete()? {
                break;
            }
            self.frames.pop_front().unwrap().rendering.wait()?;
        }
        Ok(())
    }
    fn drain(&mut self) -> Result {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !self.frames.is_empty() {
            self.retire()?;
            if std::time::Instant::now() >= deadline {
                return Err("Metal presentation completion timeout".into());
            }
            if !self.frames.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
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
        self.retire()?;
        if size.contains(&0) || size.iter().any(|&v| v > 16384) {
            return Err("unsupported drawable extent".into());
        }
        if self.drawable.is_some() {
            return Err("previous drawable was not presented".into());
        }
        if self
            .target
            .as_ref()
            .is_none_or(|(target, _)| target.size() != (size[0], size[1]))
        {
            self.layer
                .setDrawableSize(CGSize::new(size[0] as f64, size[1] as f64));
            let desc = MTLTextureDescriptor::new();
            desc.setTextureType(MTLTextureType::Type2D);
            desc.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
            // SAFETY: extent was checked against the supported device limits.
            unsafe {
                desc.setWidth(size[0] as usize);
                desc.setHeight(size[1] as usize);
            }
            desc.setStorageMode(MTLStorageMode::Private);
            desc.setHazardTrackingMode(MTLHazardTrackingMode::Tracked);
            desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
            let texture = self
                .device
                .newTextureWithDescriptor(&desc)
                .ok_or("host texture")?;
            // SAFETY: host and renderer access this allocation only inside the
            // event handoff; old frames retain their allocations across resize.
            let imported = unsafe {
                self.context.import_metal_texture(TextureDescriptor {
                    texture: texture.clone(),
                    initialized: false,
                })?
            };
            self.target = Some((imported, texture));
        }
        self.drawable = Some(
            self.layer
                .nextDrawable()
                .ok_or("Metal drawable unavailable")?,
        );
        Ok(self.target.as_ref().unwrap().0.clone())
    }
    fn target_use<'a>(&self, target: NativeRenderTarget<'a>) -> Result<NativeTargetUse<'a>> {
        let ready = self.serial.checked_add(1).ok_or("event serial overflow")?;
        // SAFETY: the preceding presentation signals serial after its last read;
        // Tileink signals ready after its last write. Zero is initially signaled.
        Ok(unsafe {
            target.with_metal_synchronization(TargetSynchronization {
                waits: vec![EventPoint {
                    event: self.event.clone(),
                    value: self.serial,
                }],
                signals: vec![EventPoint {
                    event: self.event.clone(),
                    value: ready,
                }],
            })
        })
    }
    fn present(&mut self, submission: NativeTargetSubmission) -> Result {
        let next = self.serial.checked_add(2).ok_or("event serial overflow")?;
        let drawable = self.drawable.take().ok_or("no acquired drawable")?;
        let command = self
            .queue
            .commandBuffer()
            .ok_or("presentation command buffer")?;
        command.encodeWaitForEvent_value(ProtocolObject::from_ref(&*self.event), next - 1);
        present::encode(
            &command,
            &self.pipeline,
            &self.target.as_ref().unwrap().1,
            &drawable.texture(),
        )?;
        command.encodeSignalEvent_value(ProtocolObject::from_ref(&*self.event), next);
        command.presentDrawable(ProtocolObject::from_ref(&*drawable));
        // Default Metal command buffers retain all encoded resources/drawables.
        self.frames.push_back(Frame {
            command: command.clone(),
            rendering: submission.submission,
        });
        command.commit();
        self.serial = next;
        Ok(())
    }
    fn finish(&mut self) -> Result {
        self.drain()
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Err(error) = self.drain() {
            eprintln!("Metal host teardown: {error}; retaining unconfirmed submissions");
            for frame in self.frames.drain(..) {
                std::mem::forget(frame);
            }
        }
    }
}

#[cfg(test)]
#[path = "metal/present_tests.rs"]
mod present_tests;
