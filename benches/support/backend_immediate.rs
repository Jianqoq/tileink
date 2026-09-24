use super::Gpu;

pub struct Target {
    texture: tileink::NativeTexture,
}

impl Gpu {
    pub fn target(&self, width: u32, height: u32) -> Target {
        let texture = self
            .renderer
            .context()
            .create_texture(width, height)
            .unwrap();
        Target { texture }
    }

    pub fn render_immediate(&mut self, canvas: &tileink::Canvas, target: &Target) {
        self.renderer
            .render_to_texture(canvas, &target.texture)
            .unwrap()
            .wait()
            .unwrap();
    }

    pub fn profile_immediate(&mut self, canvas: &tileink::Canvas, target: &Target) -> [u64; 2] {
        let start = std::time::Instant::now();
        let submission = self
            .renderer
            .render_to_texture(canvas, &target.texture)
            .unwrap();
        let submitted = start.elapsed();
        submission.wait().unwrap();
        [
            submitted.as_nanos() as u64,
            (start.elapsed() - submitted).as_nanos() as u64,
        ]
    }

    pub fn image_target(&self, target: &Target) -> tileink::Image {
        target.texture.readback().unwrap().readback().unwrap()
    }
}
