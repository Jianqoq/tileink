use super::Gpu;

pub struct Target {
    #[cfg(feature = "wgpu")]
    texture: wgpu::Texture,
    #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
    texture: tileink::NativeTexture,
}

impl Gpu {
    pub fn target(&self, width: u32, height: u32) -> Target {
        #[cfg(feature = "wgpu")]
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("backend comparison output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        let texture = self
            .renderer
            .context()
            .create_texture(width, height)
            .unwrap();
        Target { texture }
    }

    pub fn render_immediate(&mut self, canvas: &tileink::Canvas, target: &Target) {
        #[cfg(feature = "wgpu")]
        {
            self.renderer
                .render_to_wgpu_texture(canvas, &target.texture)
                .unwrap();
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
        }
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        self.renderer
            .render_to_texture(canvas, &target.texture)
            .unwrap()
            .wait()
            .unwrap();
    }

    pub fn profile_immediate(&mut self, canvas: &tileink::Canvas, target: &Target) -> [u64; 2] {
        let start = std::time::Instant::now();
        #[cfg(feature = "wgpu")]
        self.renderer
            .render_to_wgpu_texture(canvas, &target.texture)
            .unwrap();
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        let submission = self
            .renderer
            .render_to_texture(canvas, &target.texture)
            .unwrap();
        let submitted = start.elapsed();
        #[cfg(feature = "wgpu")]
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        submission.wait().unwrap();
        [
            submitted.as_nanos() as u64,
            (start.elapsed() - submitted).as_nanos() as u64,
        ]
    }

    pub fn image_target(&self, target: &Target) -> tileink::Image {
        #[cfg(feature = "wgpu")]
        {
            let width = target.texture.width();
            let height = target.texture.height();
            let pitch = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("benchmark readback"),
                size: u64::from(pitch) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                target.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(pitch),
                        rows_per_image: Some(height),
                    },
                },
                target.texture.size(),
            );
            self.queue.submit([encoder.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    send.send(result).unwrap()
                });
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            receive.recv().unwrap().unwrap();
            let mapped = buffer.slice(..).get_mapped_range().unwrap();
            let pixels = mapped
                .chunks_exact(pitch as usize)
                .flat_map(|row| {
                    bytemuck::cast_slice::<u8, u32>(&row[..width as usize * 4])
                        .iter()
                        .copied()
                })
                .collect();
            drop(mapped);
            buffer.unmap();
            tileink::Image {
                width,
                height,
                pixels,
            }
        }
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        target.texture.readback().unwrap().readback().unwrap()
    }
}
