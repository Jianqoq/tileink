//! WGPU execution of the shared retained sequence. Each variant owns its mutable
//! renderer, font cache, target/history, and journal cursor on the selected device.

use super::{
    Result,
    common::fonts::Snapshot,
    evidence,
    gpu::Route,
    report::Report,
    retained_sequence::{self, Frame, Sequence},
};
use peniko::Color;
use serde_json::{Value, json};
use std::path::Path;
use tileink::{
    ExternalTextureHistoryId, Image, IncrementalRenderMode, TextContext, TextFontSystem,
    WgpuRenderer,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Owned,
    Transient,
    Persistent,
}
impl Target {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Transient => "transient",
            Self::Persistent => "persistent",
        }
    }
}

struct Variant {
    name: String,
    renderer: WgpuRenderer,
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
    fn new(route: &Route, fonts: &Snapshot, kind: Target, full: bool) -> Self {
        let mut renderer = WgpuRenderer::new(
            route.renderer.device(),
            route.renderer.queue(),
            1,
            1,
            Color::TRANSPARENT,
        );
        if full {
            let mut config = renderer.incremental_render_config();
            config.mode = IncrementalRenderMode::ForceFull;
            renderer.set_incremental_render_config(config);
        }
        Self {
            name: format!(
                "{}-{}-{}",
                route.name,
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

    fn render(&mut self, sequence: &Sequence, frame: Frame) -> Result<(Image, Value)> {
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
    fn validate(&self, sequence: &Sequence, frame: Frame, image: &Image) -> Result<()> {
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

pub(super) fn validate_image(
    name: &str,
    stats: &tileink::IncrementalRenderStats,
    transient: bool,
    full: bool,
    sequence: &Sequence,
    frame: Frame,
    image: &Image,
) -> Result<()> {
    // Independent semantic checks catch shared failures that exact route comparison cannot.
    if image.rgba8_at(0, 0) != sequence.background {
        return Err(format!(
            "{} {}: background sentinel differs: {:?}",
            name,
            frame.name(),
            image.rgba8_at(0, 0)
        )
        .into());
    }
    if matches!(frame, Frame::Empty | Frame::EmptyStatic)
        && image.pixels.iter().any(|pixel| *pixel != 0)
    {
        return Err(format!(
            "{} {}: removed content left stale pixels",
            name,
            frame.name()
        )
        .into());
    }

    if matches!(frame, Frame::Geometry | Frame::Resume) {
        validate_redraw(
            stats,
            if transient {
                Target::Transient
            } else {
                Target::Owned
            },
            full,
        )
        .map_err(|error| format!("{} {}: {error}: {stats:?}", name, frame.name()))?;
    }
    if !full && !transient {
        if matches!(frame, Frame::Static | Frame::EmptyStatic)
            && (stats.dirty_tiles != 0
                || stats.chunks_rebuilt != 0
                || stats.gpu_uploaded_bytes != 0)
        {
            return Err(format!(
                "{} {}: static frame did not reuse history: {stats:?}",
                name,
                frame.name()
            )
            .into());
        }
        if frame == Frame::JournalGap && !stats.full_scene_sync {
            return Err(format!("{}: journal gap did not force full synchronization", name).into());
        }
        if frame == Frame::Resume && stats.full_scene_sync {
            return Err(format!(
                "{}: incremental updates did not resume after journal gap",
                name
            )
            .into());
        }
        if matches!(
            frame,
            Frame::Grow | Frame::Shrink | Frame::Tile15 | Frame::Tile16 | Frame::Tile17
        ) && stats.chunks_rebuilt != 0
        {
            return Err(format!("{}: resize rebuilt unchanged geometry", name).into());
        }
    }
    Ok(())
}

fn validate_redraw(
    stats: &tileink::IncrementalRenderStats,
    target: Target,
    full: bool,
) -> Result<()> {
    if full {
        if !stats.full_redraw || stats.dirty_tiles != stats.total_tiles {
            return Err("ForceFull oracle did not repaint the whole target".into());
        }
    } else if target != Target::Transient
        && (stats.full_redraw || stats.dirty_tiles == 0 || stats.dirty_tiles >= stats.total_tiles)
    {
        return Err("designated Auto frame did not perform a partial redraw".into());
    }
    Ok(())
}

pub fn render(
    fonts: &Snapshot,
    routes: &[Route],
    native: &super::native::Routes,
    output: &Path,
    validate: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut sequence = Sequence::new(fonts)?;
    let names = retained_sequence::names();
    let mut variants = Vec::new();
    let mut metadata = Vec::new();
    for route in routes {
        for kind in [Target::Owned, Target::Transient, Target::Persistent] {
            for full in [false, true] {
                let variant = Variant::new(route, fonts, kind, full);
                let mut info = route.metadata.clone();
                info["route"] = json!(variant.name);
                info["target"] = json!(kind.name());
                info["incremental_mode"] = json!(if full { "ForceFull" } else { "Auto" });
                metadata.push(info);
                variants.push(variant);
            }
        }
    }
    #[cfg(feature = "native")]
    let mut native_variants = native.retained_variants(fonts)?;
    #[cfg(feature = "native")]
    metadata.extend(
        native_variants
            .iter()
            .map(|variant| variant.metadata.clone()),
    );
    let mut report = Report::new(output, &names, metadata)?;
    let mut evidence_rows = Vec::new();
    let result: Result<()> = (|| {
        for (&frame, name) in retained_sequence::FRAMES.iter().zip(&names) {
            sequence.apply(frame)?;
            let mut images = Vec::new();
            for variant in &mut variants {
                println!("Rendering {name} through {}", variant.name);
                let (image, row) = variant.render(&sequence, frame)?;
                images.push(image);
                evidence_rows.push(row);
            }
            #[cfg(feature = "native")]
            for variant in &mut native_variants {
                println!("Rendering {name} through {}", variant.name);
                let (image, row) = variant.render(&sequence, frame)?;
                images.push(image);
                evidence_rows.push(row);
            }
            report.record(name, &images)?;
            for (variant, image) in variants.iter().zip(&images) {
                variant.validate(&sequence, frame, image)?;
            }
        }
        for (route, variants) in routes.iter().zip(variants.chunks_exact(6)) {
            for variant in variants {
                route
                    .verify_fine_compiler(variant.renderer.precompiled_dxil_pipeline_count() > 0)?;
            }
        }
        Ok(())
    })();
    evidence::write_new_json(
        &output.join("retained-pipelines-and-stats.json"),
        &json!(evidence_rows),
    )?;
    report.finish_checked(
        result
            .and_then(|()| native.validate())
            .and_then(|()| validate()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_and_force_full_execution_cannot_silently_substitute_each_other() {
        let mut stats = tileink::IncrementalRenderStats {
            dirty_tiles: 4,
            total_tiles: 64,
            ..Default::default()
        };
        assert!(validate_redraw(&stats, Target::Owned, false).is_ok());
        assert!(validate_redraw(&stats, Target::Persistent, true).is_err());
        stats.full_redraw = true;
        stats.dirty_tiles = 64;
        assert!(validate_redraw(&stats, Target::Persistent, false).is_err());
        assert!(validate_redraw(&stats, Target::Owned, true).is_ok());
        stats.full_redraw = false;
        stats.dirty_tiles = 0;
        assert!(validate_redraw(&stats, Target::Owned, false).is_err());
    }
}
