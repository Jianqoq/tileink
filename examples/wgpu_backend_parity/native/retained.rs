use crate::common::fonts::Snapshot;
use crate::retained_contract::Target;
use crate::{
    Result,
    retained_sequence::{Frame, Sequence},
};
use serde_json::{Value, json};
use tileink::{Image, IncrementalRenderMode, NativeRenderer, TextContext, TextFontSystem};

pub struct Variant {
    pub name: String,
    pub metadata: Value,
    renderer: NativeRenderer,
    fonts: TextFontSystem,
    text: TextContext,
    full: bool,
    kind: Target,
    textures: Vec<tileink::NativeTexture>,
    active: usize,
    history_epoch: u64,
}
impl Variant {
    pub fn new(
        context: &tileink::NativeContext,
        route_name: &str,
        mut metadata: Value,
        fonts: &Snapshot,
        kind: Target,
        full: bool,
    ) -> Result<Self> {
        let mut renderer = NativeRenderer::with_context(context, 1, 1)?;
        if full {
            let mut config = renderer.incremental_render_config();
            config.mode = IncrementalRenderMode::ForceFull;
            renderer.set_incremental_render_config(config);
        }
        let name = format!(
            "{route_name}-{}-{}",
            kind.name(),
            if full { "force-full" } else { "auto" }
        );
        metadata["route"] = json!(name);
        metadata["target"] = json!(kind.name());
        metadata["incremental_mode"] = json!(if full { "ForceFull" } else { "Auto" });
        Ok(Self {
            name,
            metadata,
            renderer,
            fonts: fonts.font_system(),
            text: TextContext::new(),
            full,
            kind,
            textures: Vec::new(),
            active: 0,
            history_epoch: 1,
        })
    }

    pub fn render(&mut self, sequence: &Sequence, frame: Frame) -> Result<(Image, Value)> {
        if frame == Frame::Invalidate {
            self.renderer.invalidate_retained_history();
        }
        let size = sequence.scene.physical_size();
        let image = if self.kind == Target::Owned {
            self.renderer
                .render_retained_to_image_with_text(
                    &sequence.scene,
                    &mut self.fonts,
                    &mut self.text,
                )?
                .readback()?
        } else {
            if self
                .textures
                .first()
                .is_none_or(|texture| texture.size() != size)
                || frame == Frame::ReplaceTarget
            {
                self.textures = (0..if self.kind == Target::Persistent {
                    2
                } else {
                    1
                })
                    .map(|_| self.renderer.context().create_texture(size.0, size.1))
                    .collect::<std::result::Result<_, _>>()?;
                self.active = 0;
                self.history_epoch += 1;
            }
            match frame {
                Frame::FreshHistory | Frame::ExternalClear => self.history_epoch += 1,
                Frame::SwapImage if self.kind == Target::Persistent => self.active = 1,
                Frame::ReturnImage => self.active = 0,
                _ => {}
            }
            let texture = &self.textures[self.active];
            if frame == Frame::ExternalClear {
                let mut writer =
                    NativeRenderer::with_context(self.renderer.context(), size.0, size.1)?;
                writer.set_clear_color(peniko::Color::from_rgba8(201, 71, 163, 89));
                writer
                    .render_to_texture(&tileink::Canvas::new(size.0, size.1, 1.0), texture)?
                    .wait()?;
            }
            let target = if self.kind == Target::Transient {
                tileink::NativeRenderTarget::transient(texture)
            } else {
                tileink::NativeRenderTarget::persistent(
                    texture,
                    tileink::ExternalTextureHistoryId::new(
                        self.history_epoch * 2 + self.active as u64,
                    ),
                )
            };
            self.renderer
                .render_retained_with_text_to_target(
                    &sequence.scene,
                    &mut self.fonts,
                    &mut self.text,
                    target,
                )?
                .wait()?;
            texture.readback()?.readback()?
        };
        let stats = self.renderer.incremental_render_stats();
        let row = json!({"route": self.name, "frame": frame.name(), "stats": format!("{stats:?}")});
        crate::retained_contract::validate_image(
            &self.name, &stats, false, self.full, sequence, frame, &image,
        )?;
        Ok((image, row))
    }
}
