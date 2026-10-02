// Semantic GPU tests use a dedicated target helper so they do not depend on
// performance measurement fixtures or expose those fixtures through the crate.
pub struct Gpu {
    renderer: tileink::NativeRenderer,
}

pub struct Target {
    texture: tileink::NativeTexture,
}

impl Gpu {
    pub fn new() -> Self {
        #[cfg(feature = "dx12")]
        let api = tileink::NativeBackend::Dx12;
        #[cfg(feature = "vulkan")]
        let api = tileink::NativeBackend::Vulkan;
        #[cfg(feature = "metal")]
        let api = tileink::NativeBackend::Metal;
        let context = tileink::NativeContext::new(
            api,
            &tileink::NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_TEST_GPU").expect("pin the GPU")),
                validation: false,
            },
        )
        .unwrap();
        Self {
            renderer: tileink::NativeRenderer::with_context(&context, 1280, 800).unwrap(),
        }
    }

    pub fn target(&self, width: u32, height: u32) -> Target {
        Target {
            texture: self
                .renderer
                .context()
                .create_texture(width, height)
                .unwrap(),
        }
    }

    pub fn render_immediate(&mut self, canvas: &tileink::Canvas, target: &Target) {
        self.renderer
            .render_to_texture(canvas, &target.texture)
            .unwrap()
            .wait()
            .unwrap();
    }

    pub fn image_target(&self, target: &Target) -> tileink::Image {
        target.texture.readback().unwrap().readback().unwrap()
    }
}
