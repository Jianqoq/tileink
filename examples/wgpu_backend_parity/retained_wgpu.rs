//! Wgpu execution of the shared retained sequence, reusable across platforms.
use super::{
    Result,
    common::fonts::Snapshot,
    retained_contract::{Target, validate_image},
    retained_sequence::{Frame, Sequence},
};
use peniko::Color;
use serde_json::{Value, json};
use tileink::{
    ExternalTextureHistoryId, Image, IncrementalRenderMode, TextContext, TextFontSystem,
    WgpuRenderer,
};
pub(super) struct Variant {
    pub(super) name: String,
    pub(super) renderer: WgpuRenderer,
    fonts: TextFontSystem,
    text: TextContext,
    kind: Target,
    full: bool,
    textures: Vec<wgpu::Texture>,
    active: usize,
    history_epoch: u64,
}

fn texture(device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained parity application target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl Variant {
    pub(super) fn new(
        name: &str,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &Snapshot,
        kind: Target,
        full: bool,
    ) -> Self {
        let mut renderer = WgpuRenderer::new(device, queue, 1, 1, Color::TRANSPARENT);
        if full {
            let mut config = renderer.incremental_render_config();
            config.mode = IncrementalRenderMode::ForceFull;
            renderer.set_incremental_render_config(config);
        }
        Self {
            name: format!(
                "{}-{}-{}",
                name,
                kind.name(),
                if full { "force-full" } else { "auto" }
            ),
            renderer,
            fonts: fonts.font_system(),
            text: TextContext::new(),
            kind,
            full,
            textures: Vec::new(),
            active: 0,
            history_epoch: 1,
        }
    }

    fn prepare_target(&mut self, size: (u32, u32), frame: Frame) {
        if self.kind == Target::Owned {
            return;
        }
        if self
            .textures
            .first()
            .is_none_or(|target| (target.width(), target.height()) != size)
            || frame == Frame::ReplaceTarget
        {
            let count = if self.kind == Target::Persistent {
                2
            } else {
                1
            };
            self.textures = (0..count)
                .map(|_| texture(self.renderer.device(), size))
                .collect();
            self.active = 0;
            self.history_epoch += 1;
        }
        if self.kind == Target::Persistent {
            match frame {
                Frame::FreshHistory | Frame::ExternalClear => self.history_epoch += 1,
                Frame::SwapImage => self.active = 1,
                Frame::ReturnImage => self.active = 0,
                _ => {}
            }
        }
        if frame == Frame::ExternalClear {
            let pixels: Vec<u8> = [201, 71, 163, 89].repeat(size.0 as usize * size.1 as usize);
            self.renderer.queue().write_texture(
                self.textures[self.active].as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.0 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub(super) fn render(&mut self, sequence: &Sequence, frame: Frame) -> Result<(Image, Value)> {
        let size = sequence.scene.physical_size();
        self.prepare_target(size, frame);
        match self.kind {
            Target::Owned => self.renderer.render_retained_with_text(
                &sequence.scene,
                &mut self.fonts,
                &mut self.text,
            ),
            Target::Transient => self.renderer.render_retained_with_text_to_wgpu_texture(
                &sequence.scene,
                &mut self.fonts,
                &mut self.text,
                &self.textures[self.active],
            )?,
            Target::Persistent => self
                .renderer
                .render_retained_with_text_to_persistent_wgpu_texture(
                    &sequence.scene,
                    &mut self.fonts,
                    &mut self.text,
                    &self.textures[self.active],
                    ExternalTextureHistoryId::new(self.history_epoch * 2 + self.active as u64),
                )?,
        }
        let image = if self.kind == Target::Owned {
            self.renderer.image()
        } else {
            super::readback::rgba8(
                self.renderer.device(),
                self.renderer.queue(),
                &self.textures[self.active],
            )?
        };
        let stats = self.renderer.incremental_render_stats();
        Ok((
            image,
            json!({"route": self.name, "frame": frame.name(), "stats": format!("{stats:?}"),
            "compiled_pipelines": self.renderer.pipeline_compilation_epoch(),
            "precompiled_dxil_pipelines": self.renderer.precompiled_dxil_pipeline_count()}),
        ))
    }
    pub(super) fn validate(&self, sequence: &Sequence, frame: Frame, image: &Image) -> Result<()> {
        validate_image(
            &self.name,
            self.renderer.incremental_render_stats(),
            self.kind == Target::Transient,
            self.full,
            sequence,
            frame,
            image,
        )
    }
}
