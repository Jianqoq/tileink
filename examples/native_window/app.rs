use peniko::Color;
use peniko::kurbo::{Affine, Rect};
use std::{error::Error, rc::Rc};
use tileink::{
    Canvas, NativeContext, NativeRenderTarget, NativeRenderer, NativeTargetSubmission,
    NativeTargetUse, NativeTexture, Radius, RetainedNodeId, RetainedParent, RetainedScene,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
pub trait Host {
    fn preserves_target(&self) -> bool {
        false
    }
    fn context(&self) -> &NativeContext;
    fn acquire(&mut self, size: [u32; 2]) -> Result<NativeTexture>;
    fn target_use<'a>(&self, target: NativeRenderTarget<'a>) -> Result<NativeTargetUse<'a>>;
    fn present(&mut self, submission: NativeTargetSubmission) -> Result;
}
struct State {
    // Renderer resources and imported images must be released before the host window.
    renderer: NativeRenderer,
    host: Box<dyn Host>,
    scene: RetainedScene,
    size: [u32; 2],
    window: Window,
}
struct App {
    state: Option<State>,
    backend: String,
    smoke: bool,
    frames: u32,
    error: Option<Box<dyn Error>>,
}
fn scene(size: [u32; 2]) -> Result<RetainedScene> {
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(size[0], size[1], 1.0, root)?;
    let mut canvas = Canvas::new(size[0], size[1], 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, size[0] as f64, size[1] as f64),
        Radius::ZERO,
        Color::from_rgba8(18, 25, 39, 255),
    );
    canvas.push_rect(
        Rect::new(24.0, 24.0, 160.0, 90.0),
        Radius::ZERO,
        Color::from_rgba8(41, 173, 151, 255),
    );
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(2),
            Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()?;
    Ok(scene)
}
impl App {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result {
        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title(format!("Tileink native {}", self.backend))
                .with_visible(!self.smoke)
                .with_inner_size(winit::dpi::PhysicalSize::new(640, 360)),
        )?;
        let host: Box<dyn Host> = match self.backend.as_str() {
            #[cfg(feature = "dx12")]
            "dx12" => Box::new(super::dx12::Host::new(&window)?),
            #[cfg(feature = "vulkan")]
            "vulkan" => Box::new(super::vulkan::Host::new(&window)?),
            _ => return Err("requested backend was not compiled".into()),
        };
        let size = window.inner_size();
        let size = [size.width, size.height];
        let renderer = NativeRenderer::with_context(host.context(), size[0], size[1])?;
        self.state = Some(State {
            renderer,
            host,
            scene: scene(size)?,
            size,
            window,
        });
        if self.smoke {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        }
        self.state.as_ref().unwrap().window.request_redraw();
        Ok(())
    }
    fn draw(&mut self) -> Result {
        if self.smoke && self.frames >= 8 {
            return Ok(());
        }
        let state = self.state.as_mut().unwrap();
        let extent = state.window.inner_size();
        let size = [extent.width, extent.height];
        // Minimized surfaces have no acquirable image; keep retained state intact.
        if size.contains(&0) {
            return Ok(());
        }
        if state.size != size {
            state.scene = scene(size)?;
            state.size = size;
        }
        let target = state.host.acquire(size)?;
        let target = if state.host.preserves_target() {
            NativeRenderTarget::from(&target)
        } else {
            NativeRenderTarget::transient(&target)
        };
        let usage = state.host.target_use(target)?;
        let submission = state
            .renderer
            .render_retained_to_target_use(&state.scene, usage)?;
        state.host.present(submission)?;
        state.host.context().check_validation()?;
        self.frames += 1;
        if self.smoke && self.frames == 3 {
            let _ = state
                .window
                .request_inner_size(winit::dpi::PhysicalSize::new(480, 270));
        }
        state.window.request_redraw();
        Ok(())
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Box<dyn Error>) {
        self.error = Some(error);
        event_loop.exit();
    }
}
impl ApplicationHandler for App {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Hidden smoke windows do not receive redraw events on Windows.
        if self.smoke && self.state.is_some() {
            if let Err(error) = self.draw() {
                self.fail(event_loop, error);
            }
            if self.frames >= 8 {
                event_loop.exit();
            }
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && let Err(error) = self.initialize(event_loop)
        {
            self.fail(event_loop, error);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    self.fail(event_loop, error);
                }
                if self.smoke && self.frames >= 8 {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(_) => {
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
pub fn run() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut app = App {
        state: None,
        backend: args.first().cloned().unwrap_or_else(|| {
            if cfg!(feature = "dx12") {
                "dx12".into()
            } else {
                "vulkan".into()
            }
        }),
        smoke: args.iter().any(|s| s == "--smoke"),
        frames: 0,
        error: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    println!(
        "{}: {} native present frames completed",
        app.backend, app.frames
    );
    Ok(())
}
