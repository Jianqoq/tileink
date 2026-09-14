impl super::Reference {
    pub fn render_canvas(&self, canvas: &crate::Canvas) -> super::Result<Vec<u8>> {
        let mut renderers = self.renderers.borrow_mut();
        let size = canvas.physical_size();
        let renderer = renderers.entry(size).or_insert_with(|| {
            crate::wgpu::Renderer::new(
                &self.device,
                &self.queue,
                size.0,
                size.1,
                peniko::Color::TRANSPARENT,
            )
        });
        renderer.render(canvas);
        Ok(bytemuck::cast_slice(&renderer.image().pixels).to_vec())
    }
}
