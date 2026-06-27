use ::cubecl::prelude::Runtime;
use peniko::Color;

use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        draw_record::{DrawRecord, DrawTag},
        fill::FillRule,
        image::rgba8_pack,
        line::Line,
    },
};

use super::{
    buffer::CubeBuffer,
    pipelines::{cumsum::CumsumPipeline, scan::ScanPipeline},
    types::{CubeBufferLengths, CubeSceneConfig, build_cumsum_plan, build_scan_chunks},
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

    /// Runs the CubeCL scan stage.
    ///
    /// The algorithm mirrors the CPU scan pipeline but splits it into GPU
    /// passes: clear, per-line counting, chunk-local prefix, per-path chunk
    /// offsets, offset application, and segment emission. This is a real GPU
    /// implementation, not a temporary CPU fallback.
    pub fn scan(&mut self) {
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    /// Runs the CubeCL backdrop row prefix-sum stage.
    ///
    /// This consumes scan's backdrop edge deltas and leaves cumulative backdrop
    /// values in the same GPU buffer for coarse/fine stages. The pass is fully
    /// GPU-resident; readback exists only in tests.
    pub fn cumsum(&mut self) {
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
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

pub(crate) struct SceneBuffers {
    pub(crate) line_path_ids: CubeBuffer<u32>,
    pub(crate) line_p0x: CubeBuffer<f32>,
    pub(crate) line_p0y: CubeBuffer<f32>,
    pub(crate) line_p1x: CubeBuffer<f32>,
    pub(crate) line_p1y: CubeBuffer<f32>,
    pub(crate) draw_path_ids: CubeBuffer<u32>,
    pub(crate) draw_tags: CubeBuffer<u32>,
    pub(crate) draw_fill_rules: CubeBuffer<u32>,
    pub(crate) draw_solid_rects: CubeBuffer<u32>,
    pub(crate) draw_pixel_x0: CubeBuffer<i32>,
    pub(crate) draw_pixel_y0: CubeBuffer<i32>,
    pub(crate) draw_pixel_x1: CubeBuffer<i32>,
    pub(crate) draw_pixel_y1: CubeBuffer<i32>,
    pub(crate) backdrop_data_offsets: CubeBuffer<u32>,
    pub(crate) backdrop_data_lens: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x1: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y1: CubeBuffer<u32>,
    pub(crate) backdrop_segment_starts: CubeBuffer<u32>,
    pub(crate) backdrop_segment_capacities: CubeBuffer<u32>,
    pub(crate) scan_chunk_path_ids: CubeBuffer<u32>,
    pub(crate) scan_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) scan_chunk_segment_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_lens: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_ends: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_lens: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_starts: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_ends: CubeBuffer<u32>,
}

impl SceneBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            line_path_ids: CubeBuffer::new(client, 0),
            line_p0x: CubeBuffer::new(client, 0),
            line_p0y: CubeBuffer::new(client, 0),
            line_p1x: CubeBuffer::new(client, 0),
            line_p1y: CubeBuffer::new(client, 0),
            draw_path_ids: CubeBuffer::new(client, 0),
            draw_tags: CubeBuffer::new(client, 0),
            draw_fill_rules: CubeBuffer::new(client, 0),
            draw_solid_rects: CubeBuffer::new(client, 0),
            draw_pixel_x0: CubeBuffer::new(client, 0),
            draw_pixel_y0: CubeBuffer::new(client, 0),
            draw_pixel_x1: CubeBuffer::new(client, 0),
            draw_pixel_y1: CubeBuffer::new(client, 0),
            backdrop_data_offsets: CubeBuffer::new(client, 0),
            backdrop_data_lens: CubeBuffer::new(client, 0),
            backdrop_tile_x0: CubeBuffer::new(client, 0),
            backdrop_tile_y0: CubeBuffer::new(client, 0),
            backdrop_tile_x1: CubeBuffer::new(client, 0),
            backdrop_tile_y1: CubeBuffer::new(client, 0),
            backdrop_segment_starts: CubeBuffer::new(client, 0),
            backdrop_segment_capacities: CubeBuffer::new(client, 0),
            scan_chunk_path_ids: CubeBuffer::new(client, 0),
            scan_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            scan_chunk_segment_starts: CubeBuffer::new(client, 0),
            scan_chunk_lens: CubeBuffer::new(client, 0),
            scan_chunk_range_starts: CubeBuffer::new(client, 0),
            scan_chunk_range_ends: CubeBuffer::new(client, 0),
            cumsum_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_lens: CubeBuffer::new(client, 0),
            cumsum_row_chunk_starts: CubeBuffer::new(client, 0),
            cumsum_row_chunk_ends: CubeBuffer::new(client, 0),
        }
    }

    fn upload<R: Runtime>(&mut self, client: &::cubecl::client::ComputeClient<R>, scene: &Scene) {
        let (scan_chunks, scan_chunk_ranges) = build_scan_chunks(scene);
        let cumsum_plan = build_cumsum_plan(scene);
        self.upload_lines(client, &scene.lines);
        self.upload_draws(client, &scene.draw_records);
        self.upload_backdrops(client, &scene.bd_records);

        self.scan_chunk_path_ids.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.path_id)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_backdrop_offsets.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.backdrop_offset)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_segment_starts.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.segment_start)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_lens.replace(
            client,
            &scan_chunks
                .iter()
                .map(|chunk| chunk.len)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_range_starts.replace(
            client,
            &scan_chunk_ranges
                .iter()
                .map(|range| range.start)
                .collect::<Vec<_>>(),
        );
        self.scan_chunk_range_ends.replace(
            client,
            &scan_chunk_ranges
                .iter()
                .map(|range| range.end)
                .collect::<Vec<_>>(),
        );
        self.cumsum_chunk_backdrop_offsets
            .replace(client, &cumsum_plan.chunk_backdrop_offsets);
        self.cumsum_chunk_lens
            .replace(client, &cumsum_plan.chunk_lens);
        self.cumsum_row_chunk_starts
            .replace(client, &cumsum_plan.row_chunk_starts);
        self.cumsum_row_chunk_ends
            .replace(client, &cumsum_plan.row_chunk_ends);
    }

    fn upload_lines<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lines: &[Line],
    ) {
        self.line_path_ids.replace(
            client,
            &lines.iter().map(|line| line.path_id).collect::<Vec<_>>(),
        );
        self.line_p0x.replace(
            client,
            &lines.iter().map(|line| line.p0[0]).collect::<Vec<_>>(),
        );
        self.line_p0y.replace(
            client,
            &lines.iter().map(|line| line.p0[1]).collect::<Vec<_>>(),
        );
        self.line_p1x.replace(
            client,
            &lines.iter().map(|line| line.p1[0]).collect::<Vec<_>>(),
        );
        self.line_p1y.replace(
            client,
            &lines.iter().map(|line| line.p1[1]).collect::<Vec<_>>(),
        );
    }

    fn upload_draws<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        draws: &[DrawRecord],
    ) {
        self.draw_path_ids.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.path_id.unwrap_or(u32::MAX))
                .collect::<Vec<_>>(),
        );
        self.draw_tags.replace(
            client,
            &draws
                .iter()
                .map(|draw| match draw.tag {
                    DrawTag::Brush => 0,
                    DrawTag::Clip => 1,
                    DrawTag::Opacity => 2,
                    DrawTag::Blend => 3,
                })
                .collect::<Vec<_>>(),
        );
        self.draw_fill_rules.replace(
            client,
            &draws
                .iter()
                .map(|draw| match draw.fill_rule {
                    FillRule::NonZero => 0,
                    FillRule::EvenOdd => 1,
                })
                .collect::<Vec<_>>(),
        );
        self.draw_solid_rects.replace(
            client,
            &draws
                .iter()
                .map(|draw| u32::from(draw.solid_rect))
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_x0.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.x0)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_y0.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.y0)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_x1.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.x1)
                .collect::<Vec<_>>(),
        );
        self.draw_pixel_y1.replace(
            client,
            &draws
                .iter()
                .map(|draw| draw.pixel_bounds.y1)
                .collect::<Vec<_>>(),
        );
    }

    fn upload_backdrops<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        records: &[BackdropRecord],
    ) {
        self.backdrop_data_offsets.replace(
            client,
            &records
                .iter()
                .map(|record| record.data_offset)
                .collect::<Vec<_>>(),
        );
        self.backdrop_data_lens.replace(
            client,
            &records
                .iter()
                .map(|record| record.data_len)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_x0.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_x0)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_y0.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_y0)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_x1.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_x1)
                .collect::<Vec<_>>(),
        );
        self.backdrop_tile_y1.replace(
            client,
            &records
                .iter()
                .map(|record| record.tile_y1)
                .collect::<Vec<_>>(),
        );
        self.backdrop_segment_starts.replace(
            client,
            &records
                .iter()
                .map(|record| record.segment_start)
                .collect::<Vec<_>>(),
        );
        self.backdrop_segment_capacities.replace(
            client,
            &records
                .iter()
                .map(|record| record.segment_capacity)
                .collect::<Vec<_>>(),
        );
    }
}

pub(crate) struct ScanBuffers {
    pub(crate) backdrops: CubeBuffer<i32>,
    pub(crate) tile_segment_range_starts: CubeBuffer<u32>,
    pub(crate) tile_segment_range_ends: CubeBuffer<u32>,
    pub(crate) segment_p0x: CubeBuffer<f32>,
    pub(crate) segment_p0y: CubeBuffer<f32>,
    pub(crate) segment_p1x: CubeBuffer<f32>,
    pub(crate) segment_p1y: CubeBuffer<f32>,
    pub(crate) segment_y_edge: CubeBuffer<f32>,
    pub(crate) segment_tile_counts: CubeBuffer<u32>,
    pub(crate) segment_tile_cursors: CubeBuffer<u32>,
    pub(crate) segment_bumps: CubeBuffer<u32>,
    pub(crate) chunk_totals: CubeBuffer<u32>,
    pub(crate) chunk_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_totals: CubeBuffer<i32>,
    pub(crate) cumsum_chunk_offsets: CubeBuffer<i32>,
}

impl ScanBuffers {
    fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            backdrops: CubeBuffer::new(client, 0),
            tile_segment_range_starts: CubeBuffer::new(client, 0),
            tile_segment_range_ends: CubeBuffer::new(client, 0),
            segment_p0x: CubeBuffer::new(client, 0),
            segment_p0y: CubeBuffer::new(client, 0),
            segment_p1x: CubeBuffer::new(client, 0),
            segment_p1y: CubeBuffer::new(client, 0),
            segment_y_edge: CubeBuffer::new(client, 0),
            segment_tile_counts: CubeBuffer::new(client, 0),
            segment_tile_cursors: CubeBuffer::new(client, 0),
            segment_bumps: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_totals: CubeBuffer::new(client, 0),
            cumsum_chunk_offsets: CubeBuffer::new(client, 0),
        }
    }

    fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.backdrops.resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_starts
            .resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_ends
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_p0x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p0y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_y_edge
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
        self.cumsum_chunk_totals
            .resize_uninit(client, lengths.cumsum_chunk_count);
        self.cumsum_chunk_offsets
            .resize_uninit(client, lengths.cumsum_chunk_count);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::CubeBufferLengths;
    use super::WgpuRenderer;
    use crate::cubecl::types::CUMSUM_CHUNK_SIZE;
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
        assert_eq!(lengths.cumsum_chunk_count, 0);
        assert_eq!(lengths.cumsum_row_count, 0);
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
        assert_eq!(lengths.cumsum_chunk_count, 2);
        assert_eq!(lengths.cumsum_row_count, 2);
        assert_eq!(lengths.tile_count, 4 * 3);
        assert_eq!(lengths.image_pixels, 64 * 48);
    }

    #[test]
    fn scan_wgpu_emits_one_tile_vertical_line_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(16, 16);
        scene.push_path(
            peniko::kurbo::Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        renderer.scan();

        let ranges_start = renderer
            .scan
            .tile_segment_range_starts
            .read(renderer.client());
        let ranges_end = renderer
            .scan
            .tile_segment_range_ends
            .read(renderer.client());
        let segment_bumps = renderer.scan.segment_bumps.read(renderer.client());
        let backdrops = renderer.scan.backdrops.read(renderer.client());
        let p0x = renderer.scan.segment_p0x.read(renderer.client());
        let p0y = renderer.scan.segment_p0y.read(renderer.client());
        let p1x = renderer.scan.segment_p1x.read(renderer.client());
        let p1y = renderer.scan.segment_p1y.read(renderer.client());

        assert_eq!(backdrops, vec![0]);
        assert_eq!(segment_bumps, vec![1]);
        assert_eq!(ranges_start, vec![0]);
        assert_eq!(ranges_end, vec![1]);
        assert!((p0x[0] - 4.0).abs() < 1e-3);
        assert!((p1x[0] - 4.0).abs() < 1e-3);
        assert!((p0y[0] - 0.0).abs() < 1e-6);
        assert!((p1y[0] - 16.0).abs() < 1e-6);
    }

    #[test]
    fn cumsum_wgpu_scans_backdrop_rows_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let mut scene = Scene::new(48, 32);
        scene.push_path(
            Rect::new(0.0, 0.0, 48.0, 32.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut renderer = WgpuRenderer::new_default_device(48, 32, Color::TRANSPARENT);
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer
            .scan
            .backdrops
            .replace(&client, &[1, -1, 2, 3, 0, -2]);
        renderer.cumsum();

        assert_eq!(
            renderer.scan.backdrops.read(renderer.client()),
            vec![1, 0, 2, 3, 3, 1]
        );
    }

    #[test]
    fn cumsum_wgpu_carries_across_chunks_when_enabled() {
        if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
            return;
        }

        let row_tiles = CUMSUM_CHUNK_SIZE + 3;
        let mut scene = Scene::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2);
        scene.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from(row_tiles * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE * 2),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut deltas = Vec::with_capacity((row_tiles * 2) as usize);
        let mut expected = Vec::with_capacity((row_tiles * 2) as usize);
        for row in 0..2 {
            let mut carry = 0;
            for x in 0..row_tiles {
                let value = if row == 0 {
                    1
                } else if x % 2 == 0 {
                    2
                } else {
                    -1
                };
                carry += value;
                deltas.push(value);
                expected.push(carry);
            }
        }

        let mut renderer = WgpuRenderer::new_default_device(
            row_tiles * crate::TILE_SIZE,
            crate::TILE_SIZE * 2,
            Color::TRANSPARENT,
        );
        renderer.prepare_scene(&scene);
        let client = renderer.client.clone();
        renderer.scan.backdrops.replace(&client, &deltas);
        renderer.cumsum();

        assert_eq!(renderer.scan.backdrops.read(renderer.client()), expected);
    }
}
