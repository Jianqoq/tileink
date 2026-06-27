use ::cubecl::prelude::Runtime;
use peniko::Color;

use crate::{
    scene::Scene,
    shared::{bd_record::BackdropRecord, image::rgba8_pack, line::Line, path::PathRecord},
};

use super::{
    buffer::CubeBuffer,
    types::{
        CubeBufferLengths, CubeDrawRecord, CubeLineSegment, CubeScanChunk, CubeScanChunkRange,
        CubeSceneConfig, CubeTileSegmentRange, build_scan_chunks,
    },
};

pub type WgpuRenderer = Renderer<::cubecl::wgpu::WgpuRuntime>;

/// CubeCL renderer resource owner.
///
/// This replaces the direct wgpu backend. Kernels will be written with
/// `#[cube]` and launched through CubeCL; with the current Cargo features those
/// kernels compile to the CubeCL wgpu runtime.
pub struct Renderer<R: Runtime> {
    client: ::cubecl::client::ComputeClient<R>,
    clear: Color,
    clear_color: u32,
    size: (u32, u32),
    lengths: CubeBufferLengths,
    config: CubeBuffer<CubeSceneConfig>,
    scene: SceneBuffers,
    scan: ScanBuffers,
    target: CubeBuffer<u32>,
}

impl<R: Runtime> Renderer<R> {
    pub fn new(device: &R::Device, width: u32, height: u32, clear: Color) -> Self {
        let client = R::client(device);
        let clear_color = rgba8_pack(clear.to_rgba8().to_u8_array());
        Self {
            config: CubeBuffer::new(&client, 1),
            scene: SceneBuffers::new(&client),
            scan: ScanBuffers::new(&client),
            target: CubeBuffer::new(&client, width as usize * height as usize),
            client,
            clear,
            clear_color,
            size: (width, height),
            lengths: CubeBufferLengths::default(),
        }
    }

    /// Uploads immutable scene inputs and preallocates mutable stage buffers.
    ///
    /// This is the CubeCL version of the renderer memory contract: scene input
    /// buffers are uploaded once per scene, and every compute output has fixed
    /// capacity before any kernel is launched. The actual scan/cumsum/coarse/fine
    /// kernels are intentionally added in later review stages.
    pub fn prepare_scene(&mut self, scene: &Scene) {
        self.resize(scene.width, scene.height);
        let lengths = CubeBufferLengths::from_scene(scene);
        self.lengths = lengths;
        self.scene.upload(&self.client, scene);
        self.scan.prepare_outputs(&self.client, lengths);
        self.config.replace(
            &self.client,
            &[CubeSceneConfig::new(scene, lengths, self.clear_color)],
        );
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if self.size == (width, height) {
            return;
        }
        self.size = (width, height);
        self.target
            .resize_uninit(&self.client, width as usize * height as usize);
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn clear_color(&self) -> Color {
        self.clear
    }

    pub fn buffer_lengths(&self) -> CubeBufferLengths {
        self.lengths
    }

    pub fn client(&self) -> &::cubecl::client::ComputeClient<R> {
        &self.client
    }

    pub fn runtime_name(&self) -> &'static str {
        R::name(&self.client)
    }
}

impl WgpuRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
    }
}

struct SceneBuffers {
    lines: CubeBuffer<Line>,
    path_records: CubeBuffer<PathRecord>,
    draw_records: CubeBuffer<CubeDrawRecord>,
    backdrop_records: CubeBuffer<BackdropRecord>,
    scan_chunks: CubeBuffer<CubeScanChunk>,
    scan_chunk_ranges: CubeBuffer<CubeScanChunkRange>,
}

impl SceneBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            lines: CubeBuffer::new(client, 0),
            path_records: CubeBuffer::new(client, 0),
            draw_records: CubeBuffer::new(client, 0),
            backdrop_records: CubeBuffer::new(client, 0),
            scan_chunks: CubeBuffer::new(client, 0),
            scan_chunk_ranges: CubeBuffer::new(client, 0),
        }
    }

    fn upload<R: Runtime>(&mut self, client: &::cubecl::client::ComputeClient<R>, scene: &Scene) {
        let draw_records = scene
            .draw_records
            .iter()
            .map(CubeDrawRecord::from)
            .collect::<Vec<_>>();
        let (scan_chunks, scan_chunk_ranges) = build_scan_chunks(scene);
        self.lines.replace(client, &scene.lines);
        self.path_records.replace(client, &scene.path_records);
        self.draw_records.replace(client, &draw_records);
        self.backdrop_records.replace(client, &scene.bd_records);
        self.scan_chunks.replace(client, &scan_chunks);
        self.scan_chunk_ranges.replace(client, &scan_chunk_ranges);
    }
}

struct ScanBuffers {
    backdrops: CubeBuffer<i32>,
    tile_segment_ranges: CubeBuffer<CubeTileSegmentRange>,
    segments: CubeBuffer<CubeLineSegment>,
    segment_tile_counts: CubeBuffer<u32>,
    segment_tile_cursors: CubeBuffer<u32>,
    segment_bumps: CubeBuffer<u32>,
    chunk_totals: CubeBuffer<u32>,
    chunk_offsets: CubeBuffer<u32>,
}

impl ScanBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            backdrops: CubeBuffer::new(client, 0),
            tile_segment_ranges: CubeBuffer::new(client, 0),
            segments: CubeBuffer::new(client, 0),
            segment_tile_counts: CubeBuffer::new(client, 0),
            segment_tile_cursors: CubeBuffer::new(client, 0),
            segment_bumps: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
        }
    }

    fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.backdrops.resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_ranges
            .resize_uninit(client, lengths.backdrop_len);
        self.segments
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_tile_counts
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_tile_cursors
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_bumps.resize_uninit(client, lengths.path_count);
        self.chunk_totals
            .resize_uninit(client, lengths.scan_chunk_count);
        self.chunk_offsets
            .resize_uninit(client, lengths.scan_chunk_count);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::CubeBufferLengths;
    use crate::{FillRule, Scene};

    #[test]
    fn buffer_lengths_keep_empty_scene_allocations_zero_sized_except_target() {
        let scene = Scene::new(33, 17);
        let lengths = CubeBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, 0);
        assert_eq!(lengths.path_count, 0);
        assert_eq!(lengths.backdrop_len, 0);
        assert_eq!(lengths.segment_capacity, 0);
        assert_eq!(lengths.scan_chunk_count, 0);
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
        let lengths = CubeBufferLengths::from_scene(&scene);

        assert_eq!(lengths.line_count, scene.lines.len());
        assert_eq!(lengths.path_count, scene.path_records.len());
        assert_eq!(lengths.draw_count, scene.draw_records.len());
        assert_eq!(lengths.backdrop_record_count, scene.bd_records.len());
        assert_eq!(lengths.backdrop_len, scene.backdrop_pool_capacity as usize);
        assert_eq!(lengths.segment_capacity, scene.tile_cnt as usize);
        assert_eq!(lengths.scan_chunk_count, 1);
        assert_eq!(lengths.tile_count, 4 * 3);
        assert_eq!(lengths.image_pixels, 64 * 48);
    }
}
