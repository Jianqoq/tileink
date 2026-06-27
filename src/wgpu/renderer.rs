use peniko::Color;

use crate::{
    scene::Scene,
    shared::{bd_record::BackdropRecord, image::rgba8_pack, line::Line, path::PathRecord},
};

use super::{
    buffer::{GpuImageBuffer, WgpuBuffer},
    types::{GpuLineSegment, GpuSceneConfig, GpuTileSegmentRange, WgpuBufferLengths},
};

/// WGPU renderer resource owner.
///
/// Stage 1 intentionally stops at resource ownership and preallocation. The
/// following stages will attach compute pipelines to these buffers in the same
/// order as the CPU renderer: scan, cumsum, coarse, then fine.
pub struct Renderer {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    clear: Color,
    clear_color: u32,
    size: (u32, u32),
    lengths: WgpuBufferLengths,
    config: WgpuBuffer<GpuSceneConfig>,
    scene: SceneBuffers,
    scan: ScanBuffers,
    target: GpuImageBuffer,
}

impl Renderer {
    pub fn new(
        device: ::wgpu::Device,
        queue: ::wgpu::Queue,
        width: u32,
        height: u32,
        clear: Color,
    ) -> Self {
        let clear_color = rgba8_pack(clear.to_rgba8().to_u8_array());
        Self {
            config: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                ::wgpu::BufferUsages::STORAGE,
                1,
                "tileink_wgpu_scene_config",
            ),
            scene: SceneBuffers::new(&device, &queue),
            scan: ScanBuffers::new(&device, &queue),
            target: GpuImageBuffer::new(
                device.clone(),
                queue.clone(),
                ::wgpu::BufferUsages::STORAGE,
                width,
                height,
                "tileink_wgpu_target",
            ),
            device,
            queue,
            clear,
            clear_color,
            size: (width, height),
            lengths: WgpuBufferLengths::default(),
        }
    }

    /// Uploads immutable scene inputs and preallocates scan outputs.
    ///
    /// This fixes the real GPU memory contract for later compute stages: shader
    /// code will only write into these pre-sized buffers, with no per-dispatch
    /// growth path.
    pub fn prepare_scene(&mut self, scene: &Scene) {
        self.resize(scene.width, scene.height);
        let lengths = WgpuBufferLengths::from_scene(scene);
        self.lengths = lengths;
        self.scene.upload(scene);
        self.scan.prepare_outputs(lengths);
        self.config
            .replace(&[GpuSceneConfig::new(scene, lengths, self.clear_color)]);
        self.target.clear(self.clear_color);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if self.size == (width, height) {
            return;
        }
        self.size = (width, height);
        self.target.resize_zeroed(width, height);
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn clear_color(&self) -> Color {
        self.clear
    }

    pub fn buffer_lengths(&self) -> WgpuBufferLengths {
        self.lengths
    }

    pub fn device(&self) -> &::wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &::wgpu::Queue {
        &self.queue
    }

    pub fn target_buffer(&self) -> &::wgpu::Buffer {
        self.target.buffer()
    }
}

struct SceneBuffers {
    lines: WgpuBuffer<Line>,
    path_records: WgpuBuffer<PathRecord>,
    backdrop_records: WgpuBuffer<BackdropRecord>,
}

impl SceneBuffers {
    fn new(device: &::wgpu::Device, queue: &::wgpu::Queue) -> Self {
        let usage = ::wgpu::BufferUsages::STORAGE;
        Self {
            lines: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_lines",
            ),
            path_records: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_path_records",
            ),
            backdrop_records: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_backdrop_records",
            ),
        }
    }

    fn upload(&mut self, scene: &Scene) {
        self.lines.replace(&scene.lines);
        self.path_records.replace(&scene.path_records);
        self.backdrop_records.replace(&scene.bd_records);
    }
}

struct ScanBuffers {
    backdrops: WgpuBuffer<i32>,
    tile_segment_ranges: WgpuBuffer<GpuTileSegmentRange>,
    segments: WgpuBuffer<GpuLineSegment>,
    segment_tile_counts: WgpuBuffer<u32>,
    segment_tile_cursors: WgpuBuffer<u32>,
    segment_bumps: WgpuBuffer<u32>,
}

impl ScanBuffers {
    fn new(device: &::wgpu::Device, queue: &::wgpu::Queue) -> Self {
        let usage = ::wgpu::BufferUsages::STORAGE;
        Self {
            backdrops: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_backdrops",
            ),
            tile_segment_ranges: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_tile_segment_ranges",
            ),
            segments: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_segments",
            ),
            segment_tile_counts: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_segment_tile_counts",
            ),
            segment_tile_cursors: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_segment_tile_cursors",
            ),
            segment_bumps: WgpuBuffer::new(
                device.clone(),
                queue.clone(),
                usage,
                0,
                "tileink_wgpu_segment_bumps",
            ),
        }
    }

    fn prepare_outputs(&mut self, lengths: WgpuBufferLengths) {
        self.backdrops.resize_zeroed_all(lengths.backdrop_len);
        self.tile_segment_ranges
            .resize_zeroed_all(lengths.backdrop_len);
        self.segments.resize_zeroed(lengths.segment_capacity);
        self.segment_tile_counts
            .resize_zeroed_all(lengths.backdrop_len);
        self.segment_tile_cursors
            .resize_zeroed_all(lengths.backdrop_len);
        self.segment_bumps.resize_zeroed_all(lengths.path_count);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::WgpuBufferLengths;
    use crate::{FillRule, Scene};

    #[test]
    fn buffer_lengths_keep_empty_scene_allocations_zero_sized_except_target() {
        let scene = Scene::new(33, 17);
        let lengths = WgpuBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, 0);
        assert_eq!(lengths.path_count, 0);
        assert_eq!(lengths.backdrop_len, 0);
        assert_eq!(lengths.segment_capacity, 0);
        assert_eq!(lengths.tile_count, 6);
        assert_eq!(lengths.image_pixels, 33 * 17);
    }

    #[test]
    fn buffer_lengths_match_scene_preallocated_path_storage() {
        let mut scene = Scene::new(64, 48);
        scene.push_path(
            Rect::new(8.0, 8.0, 40.0, 32.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        let lengths = WgpuBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, scene.lines.len());
        assert_eq!(lengths.path_count, scene.path_records.len());
        assert_eq!(lengths.draw_count, scene.draw_records.len());
        assert_eq!(lengths.backdrop_record_count, scene.bd_records.len());
        assert_eq!(lengths.backdrop_len, scene.backdrop_pool_capacity as usize);
        assert_eq!(lengths.segment_capacity, scene.tile_cnt as usize);
        assert_eq!(lengths.tile_count, 4 * 3);
        assert_eq!(lengths.image_pixels, 64 * 48);
    }
}
