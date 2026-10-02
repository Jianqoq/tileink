use super::platform::{Size, Window};
use peniko::Color;
use peniko::kurbo::{Affine, Rect};
use std::{error::Error, rc::Rc};
use tileink::{
    Canvas, NativeContext, NativeRenderTarget, NativeRenderer, NativeTargetSubmission,
    NativeTargetUse, NativeTexture, Radius, RetainedNodeId, RetainedParent, RetainedScene,
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
    fn finish(&mut self) -> Result {
        Ok(())
    }
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
    sizes: Vec<[u32; 2]>,
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
    fn initialize(&mut self) -> Result {
        let window = Window::new(&format!("Tileink native {}", self.backend), !self.smoke)?;
        let host: Box<dyn Host> = match self.backend.as_str() {
            #[cfg(all(target_os = "macos", feature = "metal"))]
            "metal" => Box::new(super::metal::Host::new(&window)?),
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
        if self.sizes.last() != Some(&size) {
            self.sizes.push(size);
        }
        if self.smoke && self.frames == 3 {
            state.window.request_inner_size(Size {
                width: 480,
                height: 270,
            });
        }
        Ok(())
    }
}
pub fn run() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut app = App {
        state: None,
        backend: args.first().cloned().unwrap_or_else(|| {
            if cfg!(feature = "metal") {
                "metal".into()
            } else if cfg!(feature = "dx12") {
                "dx12".into()
            } else {
                "vulkan".into()
            }
        }),
        smoke: args.iter().any(|s| s == "--smoke"),
        frames: 0,
        sizes: Vec::new(),
    };
    app.initialize()?;
    loop {
        let visible = app.state.as_ref().unwrap().window.pump();
        if !app.smoke && !visible {
            break;
        }
        app.draw()?;
        if app.smoke && app.frames >= 8 {
            break;
        }
    }
    if let Some(state) = &mut app.state {
        state.host.finish()?;
    }
    if app.smoke && (app.frames != 8 || app.sizes.len() < 2) {
        return Err(format!(
            "smoke did not complete eight frames and a real resize: {:?}",
            app.sizes
        )
        .into());
    }
    println!(
        "{}: {} native present frames completed; physical sizes {:?}",
        app.backend, app.frames, app.sizes
    );
    Ok(())
}
