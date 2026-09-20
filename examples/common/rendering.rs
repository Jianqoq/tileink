//! One scene callback shared by wgpu and native example capture.
use peniko::Color;
use tileink::{Canvas, Image, TextContext, TextFontSystem};

#[cfg(feature = "wgpu")]
use tileink::WgpuRenderer;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub trait SceneRenderer {
    fn render(&mut self, canvas: &Canvas) -> Result<()>;
    fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<()>;
}

#[cfg(feature = "wgpu")]
impl SceneRenderer for WgpuRenderer {
    fn render(&mut self, canvas: &Canvas) -> Result<()> {
        WgpuRenderer::render(self, canvas);
        Ok(())
    }
    fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<()> {
        WgpuRenderer::render_with_text(self, canvas, fonts, text);
        Ok(())
    }
}

pub(super) enum Backend {
    #[cfg(feature = "wgpu")]
    Wgpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
    },
    #[cfg(tileink_native_runtime)]
    Native(tileink::NativeContext),
}

impl Backend {
    pub fn create(&self, width: u32, height: u32) -> Result<Renderer> {
        Ok(match self {
            #[cfg(feature = "wgpu")]
            Self::Wgpu { device, queue } => Renderer::Wgpu(Box::new(WgpuRenderer::new(
                device,
                queue,
                width,
                height,
                Color::TRANSPARENT,
            ))),
            #[cfg(tileink_native_runtime)]
            Self::Native(context) => Renderer::Native(Box::new(NativeFrame {
                renderer: tileink::NativeRenderer::with_context(context, width, height)?,
                image: None,
            })),
        })
    }
}

pub(super) enum Renderer {
    #[cfg(feature = "wgpu")]
    Wgpu(Box<WgpuRenderer>),
    #[cfg(tileink_native_runtime)]
    Native(Box<NativeFrame>),
}

impl Renderer {
    pub fn scene_renderer(&mut self) -> &mut dyn SceneRenderer {
        match self {
            #[cfg(feature = "wgpu")]
            Self::Wgpu(renderer) => renderer.as_mut(),
            #[cfg(tileink_native_runtime)]
            Self::Native(frame) => frame.as_mut(),
        }
    }
    pub fn set_clear_color(&mut self, clear: Color) {
        match self {
            #[cfg(feature = "wgpu")]
            Self::Wgpu(renderer) => renderer.set_clear_color(clear),
            #[cfg(tileink_native_runtime)]
            Self::Native(frame) => frame.renderer.set_clear_color(clear),
        }
    }
    pub fn image(&mut self) -> Result<Image> {
        match self {
            #[cfg(feature = "wgpu")]
            Self::Wgpu(renderer) => Ok(renderer.image()),
            #[cfg(tileink_native_runtime)]
            Self::Native(frame) => frame
                .image
                .take()
                .ok_or_else(|| "example callback did not render an image".into()),
        }
    }
}

#[cfg(tileink_native_runtime)]
pub(super) struct NativeFrame {
    renderer: tileink::NativeRenderer,
    image: Option<Image>,
}

#[cfg(tileink_native_runtime)]
impl SceneRenderer for NativeFrame {
    fn render(&mut self, canvas: &Canvas) -> Result<()> {
        self.image = Some(self.renderer.render_to_image(canvas)?.readback()?);
        Ok(())
    }
    fn render_with_text(
        &mut self,
        canvas: &Canvas,
        fonts: &mut TextFontSystem,
        text: &mut TextContext,
    ) -> Result<()> {
        self.image = Some(
            self.renderer
                .render_to_image_with_text(canvas, fonts, text)?
                .readback()?,
        );
        Ok(())
    }
}
