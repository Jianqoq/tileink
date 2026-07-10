//! Caller-owned texture presentation and renderer target readback.
//!
//! These APIs choose where retained history lives, but delegate frame planning and cache
//! ownership to `RetainedRenderState` instead of adding another state machine to `Renderer`.

use std::sync::mpsc;

use crate::{Canvas, TextFontSystem, shared::image::Image, text::TextContext};

use super::{
    super::{
        fine::WgpuFinePipeline,
        incremental::{IncrementalOutputMode, TransientOutputDecision},
    },
    Renderer,
    retained::HistoryOwner,
};

#[derive(Debug)]
pub enum WgpuTextureRenderError {
    DestinationTooSmall {
        required_width: u32,
        required_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    DestinationUsageMissing(::wgpu::TextureUsages),
    DestinationStorageUsageMissing(::wgpu::TextureUsages),
    UnsupportedDestination {
        format: ::wgpu::TextureFormat,
        dimension: ::wgpu::TextureDimension,
        sample_count: u32,
    },
}

/// Stable identity for a caller-owned texture whose pixels persist between retained frames.
///
/// Reuse an ID only while passing the same texture with unmodified contents. Allocate a new ID
/// after recreating, resizing, clearing, or otherwise mutating that texture outside tileink.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExternalTextureHistoryId(u64);

impl ExternalTextureHistoryId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for WgpuTextureRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DestinationTooSmall {
                required_width,
                required_height,
                actual_width,
                actual_height,
            } => write!(
                f,
                "destination texture is {actual_width}x{actual_height}, but {required_width}x{required_height} is required"
            ),
            Self::DestinationUsageMissing(usage) => write!(
                f,
                "destination texture usage {usage:?} is missing wgpu::TextureUsages::COPY_DST"
            ),
            Self::DestinationStorageUsageMissing(usage) => write!(
                f,
                "destination texture usage {usage:?} is missing wgpu::TextureUsages::STORAGE_BINDING"
            ),
            Self::UnsupportedDestination {
                format,
                dimension,
                sample_count,
            } => write!(
                f,
                "unsupported destination texture format {format:?}, dimension {dimension:?}, sample_count {sample_count}; expected single-sample 2D Rgba8Unorm or Rgba8UnormSrgb"
            ),
        }
    }
}

impl std::error::Error for WgpuTextureRenderError {}

#[derive(Clone, Copy)]
enum RequestedTextureHistory {
    Transient,
    External(ExternalTextureHistoryId),
}

impl Renderer {
    pub fn image(&self) -> Image {
        let byte_len = self.target_rgba8_byte_len();
        if byte_len == 0 {
            return Image {
                width: self.size.0,
                height: self.size.1,
                pixels: Vec::new(),
            };
        }

        let row_bytes = self.target_row_bytes();
        let padded_row_bytes =
            row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
        let readback = self.device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu target readback"),
            size: padded_row_bytes * self.size.1 as u64,
            usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&::wgpu::CommandEncoderDescriptor {
                label: Some("tileink wgpu target readback copy"),
            });
        encoder.copy_texture_to_buffer(
            self.readback_target.texture().as_image_copy(),
            ::wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: ::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes as u32),
                    rows_per_image: None,
                },
            },
            self.target_texture_extent(),
        );
        self.queue.submit([encoder.finish()]);

        let (tx, rx) = mpsc::channel();
        readback
            .slice(..)
            .map_async(::wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap()
            });
        self.device
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("poll wgpu device for target readback");
        rx.recv()
            .expect("receive target readback map result")
            .expect("map wgpu target readback buffer");

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .expect("read mapped wgpu target readback buffer");
        let mut pixels = Vec::with_capacity(self.size.0 as usize * self.size.1 as usize);
        for row in 0..self.size.1 as usize {
            let start = row * padded_row_bytes as usize;
            let row = &mapped[start..start + row_bytes as usize];
            pixels.extend_from_slice(bytemuck::cast_slice(row));
        }
        drop(mapped);
        readback.unmap();
        Image {
            width: self.size.0,
            height: self.size.1,
            pixels,
        }
    }

    pub fn device(&self) -> &::wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &::wgpu::Queue {
        &self.queue
    }

    pub fn target_rgba8_byte_len(&self) -> ::wgpu::BufferAddress {
        rgba8_byte_len(self.size.0, self.size.1)
    }

    pub fn render_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture(canvas, dst, RequestedTextureHistory::Transient)?;
        self.last_frame_used_native = true;
        Ok(())
    }

    /// Renders into a caller-owned texture that remains intact between retained frames.
    ///
    /// The destination itself becomes active root history, so this path does not update or resize
    /// renderer-owned root history and does not issue a full-surface presentation copy.
    /// `history_id` must change if the destination is recreated or externally modified.
    pub fn render_to_persistent_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
        history_id: ExternalTextureHistoryId,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture(
            canvas,
            dst,
            RequestedTextureHistory::External(history_id),
        )?;
        self.last_frame_used_native = true;
        Ok(())
    }

    pub fn render_with_text_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_with_text_to_wgpu_texture(
            canvas,
            font_system,
            text_context,
            dst,
            RequestedTextureHistory::Transient,
        )?;
        self.last_frame_used_native = true;
        Ok(())
    }

    /// Text-capable variant of [`Self::render_to_persistent_wgpu_texture`].
    pub fn render_with_text_to_persistent_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
        history_id: ExternalTextureHistoryId,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_with_text_to_wgpu_texture(
            canvas,
            font_system,
            text_context,
            dst,
            RequestedTextureHistory::External(history_id),
        )?;
        self.last_frame_used_native = true;
        Ok(())
    }

    fn render_native_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
        history: RequestedTextureHistory,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture_with_prepare(
            canvas,
            dst,
            false,
            history,
            |renderer, canvas| renderer.prepare_scene(canvas),
        )
    }

    fn render_native_with_text_to_wgpu_texture(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
        dst: &::wgpu::Texture,
        history: RequestedTextureHistory,
    ) -> Result<(), WgpuTextureRenderError> {
        self.render_native_to_wgpu_texture_with_prepare(
            canvas,
            dst,
            true,
            history,
            |renderer, canvas| {
                renderer.prepare_scene_with_text(canvas, font_system, text_context);
            },
        )
    }

    fn render_native_to_wgpu_texture_with_prepare(
        &mut self,
        canvas: &Canvas,
        dst: &::wgpu::Texture,
        uses_text: bool,
        requested_history: RequestedTextureHistory,
        prepare: impl FnOnce(&mut Self, &Canvas),
    ) -> Result<(), WgpuTextureRenderError> {
        self.validate_wgpu_storage_texture_destination(
            dst,
            canvas.physical_width(),
            canvas.physical_height(),
        )?;
        let selected = self.retained.select_scene(canvas);
        let frame = selected.frame();
        let retained_ptr = selected.retained_ptr();
        let materialized_reused = selected.materialized_reused();
        let is_retained = frame.is_some();
        let history_owner = match (is_retained, requested_history) {
            (true, RequestedTextureHistory::External(id)) => HistoryOwner::External(id),
            _ => HistoryOwner::Internal,
        };
        self.retained.set_history_owner(history_owner);
        let transient_without_copy = is_retained
            && matches!(requested_history, RequestedTextureHistory::Transient)
            && !dst.usage().contains(::wgpu::TextureUsages::COPY_DST);
        if transient_without_copy {
            // A swapchain without COPY_DST cannot receive internal history. Keep the retained
            // scene baseline for damage statistics, but force full direct output for correctness.
            self.retained.invalidate_history();
            self.retained.reset_transient_output();
        }
        let scene = selected.scene();
        let plan = self
            .retained
            .begin_frame(frame, scene, self.profiler.is_active());
        self.retained.stats_mut().materialized_scene_reused = materialized_reused;
        let (output_mode, history_updated, copy_history) = if !is_retained {
            self.retained.reset_transient_output();
            (IncrementalOutputMode::DirectTransient, false, false)
        } else {
            match requested_history {
                RequestedTextureHistory::External(_) => {
                    (IncrementalOutputMode::ExternalHistory, true, false)
                }
                RequestedTextureHistory::Transient if transient_without_copy => {
                    (IncrementalOutputMode::DirectTransient, false, false)
                }
                RequestedTextureHistory::Transient => {
                    match self.retained.decide_transient_output(&plan.stats) {
                        TransientOutputDecision::Direct => {
                            (IncrementalOutputMode::DirectTransient, false, false)
                        }
                        TransientOutputDecision::RebuildHistory => {
                            (IncrementalOutputMode::RebuildHistory, true, true)
                        }
                        TransientOutputDecision::InternalHistory => {
                            (IncrementalOutputMode::InternalHistory, true, true)
                        }
                    }
                }
            }
        };
        let render_direct = matches!(
            output_mode,
            IncrementalOutputMode::DirectTransient | IncrementalOutputMode::ExternalHistory
        );
        if render_direct {
            self.root_target_texture = Some(dst.clone());
            self.root_target_view =
                Some(dst.create_view(&::wgpu::TextureViewDescriptor::default()));
        } else {
            // A direct resize deliberately leaves internal history at its old allocation. The
            // retained scene may still be prepared when hysteresis later chooses RebuildHistory,
            // so target allocation cannot be coupled only to scene-buffer preparation.
            self.readback_target.resize(
                &self.device,
                scene.physical_width(),
                scene.physical_height(),
            );
        }
        self.retained.stats_mut().output_mode = output_mode;
        let has_work = !plan.tiles.is_empty();
        if has_work
            && self.retained.scene_needs_prepare(
                retained_ptr,
                uses_text,
                self.image_resources_dirty,
            )
        {
            prepare(self, scene);
            self.retained.mark_scene_prepared(retained_ptr, uses_text);
        }
        let rendered = if has_work || copy_history {
            self.render_prepared_tile_plan_with_history_copy(scene, copy_history.then_some(dst))
        } else {
            true
        };
        self.root_target_view = None;
        self.root_target_texture = None;
        self.retained.finish_frame(plan, rendered, history_updated);
        if rendered {
            self.size = scene.physical_size();
            if copy_history {
                self.retained.stats_mut().history_copied_to_output = true;
            }
            return Ok(());
        }
        panic!("wgpu renderer could not render scene natively")
    }

    fn validate_wgpu_storage_texture_destination(
        &self,
        dst: &::wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuTextureRenderError> {
        if dst.width() < width || dst.height() < height {
            return Err(WgpuTextureRenderError::DestinationTooSmall {
                required_width: width,
                required_height: height,
                actual_width: dst.width(),
                actual_height: dst.height(),
            });
        }
        if !dst.usage().contains(::wgpu::TextureUsages::STORAGE_BINDING) {
            return Err(WgpuTextureRenderError::DestinationStorageUsageMissing(
                dst.usage(),
            ));
        }
        if self
            .fine
            .as_ref()
            .is_some_and(WgpuFinePipeline::uses_portable_textures)
            && !dst
                .usage()
                .contains(::wgpu::TextureUsages::COPY_SRC | ::wgpu::TextureUsages::COPY_DST)
        {
            return Err(WgpuTextureRenderError::DestinationUsageMissing(dst.usage()));
        }
        if dst.format() != ::wgpu::TextureFormat::Rgba8Unorm
            || dst.dimension() != ::wgpu::TextureDimension::D2
            || dst.sample_count() != 1
        {
            return Err(WgpuTextureRenderError::UnsupportedDestination {
                format: dst.format(),
                dimension: dst.dimension(),
                sample_count: dst.sample_count(),
            });
        }
        Ok(())
    }

    fn target_row_bytes(&self) -> ::wgpu::BufferAddress {
        self.size.0 as ::wgpu::BufferAddress * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
    }

    fn target_texture_extent(&self) -> ::wgpu::Extent3d {
        ::wgpu::Extent3d {
            width: self.size.0,
            height: self.size.1,
            depth_or_array_layers: 1,
        }
    }
}

fn rgba8_byte_len(width: u32, height: u32) -> ::wgpu::BufferAddress {
    width as ::wgpu::BufferAddress
        * height as ::wgpu::BufferAddress
        * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
}
