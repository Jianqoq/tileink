use ::cubecl::prelude::Runtime;
use peniko::Color;

mod executor;
mod filter_cursors;
mod filter_resources;
mod resources;
mod scratch;
mod target;
#[cfg(feature = "profile")]
use super::profile::{
    RenderProfile, RenderProfileMemoryEntry, RenderProfileMemorySpace, RenderProfiler,
    finish_profile_scope, start_profile_scope, sync_client,
};
use filter_resources::{
    FilterConvolveBuffers, FilterConvolveUpload, FilterPathBuffers, FilterPathUpload,
    FilterTransferBuffers, FilterTransferUpload, FilterTurbulenceBuffers, FilterTurbulenceUpload,
};
use resources::SceneUploadStaging;
pub(crate) use resources::{CoarseBuffers, ScanBuffers, SceneBuffers};
use scratch::{plan_stack_depths, required_scratch_count};
use target::CubeRenderTarget;

use crate::{
    debug::{DebugScanBuffers, RenderDebugCapture, RenderOptions, capture_render_debug},
    render::Render,
    scene::Scene,
    shared::{
        execution::{ExecPlan, ROOT_COMMAND_LIST_ID},
        image::{Image, premul_color_to_rgba8_pack},
        line_seg::LineSegment,
        tile_seg_range::TileSegmentRange,
    },
    text::{PreparedTextData, TextContext},
};

use super::{
    brush::{GpuBrushBuffers, GpuBrushUpload},
    buffer::CubeBuffer,
    pipelines::{
        coarse::{CoarseBatch, CoarsePipeline},
        cumsum::CumsumPipeline,
        fine::{
            FINE_GROUP_SPILL_FIELDS, FINE_LOCAL_CLIP_DEPTH, FINE_LOCAL_GROUP_DEPTH,
            FineOutputBuffers, FinePipeline, FineRenderConfig,
        },
        scan::ScanPipeline,
    },
    types::{CubeBufferLengths, CubeSceneConfig},
};
#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

pub type WgpuRenderer = Renderer<::cubecl::wgpu::WgpuRuntime>;
#[cfg(feature = "cuda")]
pub type CudaRenderer = Renderer<::cubecl::cuda::CudaRuntime>;

/// CubeCL renderer resource owner.
///
/// This replaces the direct wgpu backend. Kernels will be written with
/// `#[cube]` and launched through CubeCL; with the current Cargo features those
/// kernels compile to the CubeCL wgpu runtime.
pub struct Renderer<R: Runtime> {
    client: ::cubecl::client::ComputeClient<R>,
    clear_color: u32,
    size: (u32, u32),
    lengths: CubeBufferLengths,
    max_clip_depth: usize,
    max_group_depth: usize,
    plan: Option<ExecPlan>,
    config: CubeBuffer<CubeSceneConfig>,
    scene: SceneBuffers,
    scan: ScanBuffers,
    coarse: CoarseBuffers,
    scene_upload: SceneUploadStaging,
    draw_brushes: GpuBrushBuffers,
    filter_brushes: GpuBrushBuffers,
    filter_convolves: FilterConvolveBuffers,
    filter_paths: FilterPathBuffers,
    filter_transfers: FilterTransferBuffers,
    filter_turbulence: FilterTurbulenceBuffers,
    text_data: Option<PreparedTextData>,
    target: CubeBuffer<u32>,
    fine_clip_spills: CubeBuffer<u32>,
    fine_group_spills: CubeBuffer<u32>,
    scratch: Vec<CubeBuffer<u32>>,
    scratch_in_use: Vec<bool>,
    surface_sources: Vec<CubeBuffer<u32>>,
    surface_origin: (i32, i32),
    #[cfg(feature = "profile")]
    profiler: RenderProfiler,
}

#[cfg(feature = "bench-api")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubePreparedStage {
    Scan,
    Cumsum,
    Coarse,
    Fine,
}

#[cfg(feature = "wgpu")]
#[derive(Debug)]
pub enum WgpuTextureBlitError {
    DestinationTooSmall {
        required_width: u32,
        required_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    DestinationUsageMissing(wgpu::TextureUsages),
    UnsupportedDestination {
        format: wgpu::TextureFormat,
        dimension: wgpu::TextureDimension,
        sample_count: u32,
    },
    SourceTooSmall {
        required: wgpu::BufferAddress,
        available: wgpu::BufferAddress,
    },
    SourceUnavailable(::cubecl::server::ServerError),
}

#[cfg(feature = "wgpu")]
impl std::fmt::Display for WgpuTextureBlitError {
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
            Self::UnsupportedDestination {
                format,
                dimension,
                sample_count,
            } => write!(
                f,
                "unsupported destination texture format {format:?}, dimension {dimension:?}, sample_count {sample_count}; expected single-sample 2D Rgba8Unorm or Rgba8UnormSrgb"
            ),
            Self::SourceTooSmall {
                required,
                available,
            } => write!(
                f,
                "CubeCL target resource has {available} bytes, but {required} bytes are required"
            ),
            Self::SourceUnavailable(err) => {
                write!(f, "CubeCL target resource is unavailable: {err}")
            }
        }
    }
}

#[cfg(feature = "wgpu")]
impl std::error::Error for WgpuTextureBlitError {}

impl<R: Runtime> Render for Renderer<R> {
    type ScanArgs<'a> = ();
    type CumsumArgs<'a> = ();
    type CoarseArgs<'a> = CoarseBatch;
    type ExecuteArgs<'a> = ();

    fn render(&mut self, scene: &Scene) {
        self.prepare_scene(scene);
        <Self as Render>::scan(self, scene, ());
        <Self as Render>::cumsum(self, scene, ());
        self.clear_target();
        self.execute_prepared_plan(scene);
    }

    fn execute(&mut self, scene: &Scene, _: Self::ExecuteArgs<'_>) {
        <Self as Render>::render(self, scene);
    }

    fn scan(&mut self, _: &Scene, _: Self::ScanArgs<'_>) {
        self.run_scan_pipeline();
    }

    fn cumsum(&mut self, _: &Scene, _: Self::CumsumArgs<'_>) {
        self.run_cumsum_pipeline();
    }

    fn coarse(&mut self, _: &Scene, batch: Self::CoarseArgs<'_>) {
        self.run_coarse_pipeline(batch);
    }
}

impl<R: Runtime> Renderer<R> {
    fn run_scan_pipeline(&mut self) {
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    fn run_cumsum_pipeline(&mut self) {
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
    }

    fn run_coarse_pipeline(&mut self, batch: CoarseBatch) {
        CoarsePipeline::run(
            &self.client,
            &self.scene,
            &self.scan,
            &mut self.coarse,
            self.lengths,
            batch,
        );
    }

    pub fn new(device: &R::Device, width: u32, height: u32, clear: Color) -> Self {
        let client = R::client(device);
        let clear_color = premul_color_to_rgba8_pack(clear);
        Self {
            config: CubeBuffer::new(&client, 1),
            scene: SceneBuffers::new(&client),
            scan: ScanBuffers::new(&client),
            coarse: CoarseBuffers::new(&client),
            scene_upload: SceneUploadStaging::default(),
            draw_brushes: GpuBrushBuffers::new(&client),
            filter_brushes: GpuBrushBuffers::new(&client),
            filter_convolves: FilterConvolveBuffers::new(&client),
            filter_paths: FilterPathBuffers::new(&client),
            filter_transfers: FilterTransferBuffers::new(&client),
            filter_turbulence: FilterTurbulenceBuffers::new(&client),
            text_data: None,
            target: CubeBuffer::new(&client, width as usize * height as usize),
            fine_clip_spills: CubeBuffer::new(&client, 0),
            fine_group_spills: CubeBuffer::new(&client, 0),
            scratch: Vec::new(),
            scratch_in_use: Vec::new(),
            surface_sources: Vec::new(),
            surface_origin: (0, 0),
            #[cfg(feature = "profile")]
            profiler: RenderProfiler::default(),
            client,
            clear_color,
            size: (width, height),
            lengths: CubeBufferLengths::default(),
            max_clip_depth: 0,
            max_group_depth: 0,
            plan: None,
        }
    }

    /// Uploads immutable scene inputs and preallocates mutable stage buffers.
    ///
    /// This is the CubeCL version of the renderer memory contract: scene input
    /// buffers are uploaded once per scene, and every compute output has fixed
    /// capacity before any kernel is launched.
    fn prepare_scene(&mut self, scene: &Scene) {
        #[cfg(feature = "profile")]
        let timer = start_profile_scope("prepare_scene");
        self.text_data = None;
        self.prepare_scene_resources(scene);
        #[cfg(feature = "profile")]
        finish_profile_scope(&self.client, timer);
    }

    fn prepare_scene_with_text(&mut self, scene: &Scene, text_context: &mut TextContext) {
        #[cfg(feature = "profile")]
        let timer = start_profile_scope("prepare_scene");
        self.text_data = Some(PreparedTextData::new(
            &scene.text_glyphs,
            &scene.text_runs,
            text_context,
        ));
        self.prepare_scene_resources(scene);
        #[cfg(feature = "profile")]
        finish_profile_scope(&self.client, timer);
    }

    fn prepare_scene_resources(&mut self, scene: &Scene) {
        self.surface_sources.clear();
        self.surface_origin = (0, 0);
        self.resize(scene.width, scene.height);
        let lengths = CubeBufferLengths::from_scene_with_text(scene, self.text_data.as_ref());
        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        let (max_clip_depth, max_group_depth) = plan_stack_depths(&plan);
        let scratch_count = required_scratch_count(&plan);
        let filter_brush_upload = GpuBrushUpload::from_filter_plan(&plan.ops);
        let filter_convolve_upload = FilterConvolveUpload::from_plan(&plan);
        let filter_path_upload = FilterPathUpload::from_plan(&plan);
        let filter_transfer_upload = FilterTransferUpload::from_plan(&plan);
        let filter_turbulence_upload = FilterTurbulenceUpload::from_plan(&plan);
        self.lengths = lengths;
        self.max_clip_depth = max_clip_depth;
        self.max_group_depth = max_group_depth;
        self.prepare_fine_stack_spills(lengths, max_clip_depth, max_group_depth);
        self.prepare_scratch_buffers(scratch_count);
        self.draw_brushes
            .upload(&self.client, &scene.columns.draw_brushes);
        self.filter_brushes
            .upload(&self.client, &filter_brush_upload);
        self.filter_convolves
            .upload(&self.client, filter_convolve_upload);
        self.filter_paths.upload(&self.client, filter_path_upload);
        self.filter_transfers
            .upload(&self.client, filter_transfer_upload);
        self.filter_turbulence
            .upload(&self.client, filter_turbulence_upload);
        self.scene.upload(
            &self.client,
            scene,
            &plan,
            self.text_data.as_ref(),
            &mut self.scene_upload,
        );
        self.scan.prepare_outputs(&self.client, lengths);
        self.coarse.prepare_outputs(&self.client, lengths);
        self.config.replace(
            &self.client,
            &[CubeSceneConfig::new(scene, lengths, self.clear_color)],
        );
        self.plan = Some(plan);
    }

    fn coarse_batch(
        &mut self,
        scene: &Scene,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
    ) {
        <Self as Render>::coarse(
            self,
            scene,
            CoarseBatch {
                draw_start,
                draw_end,
                layer_stack_start,
                layer_stack_end,
            },
        );
    }

    fn clear_target(&mut self) {
        self.clear_buffer(CubeRenderTarget::Main, self.clear_color);
    }

    fn fine_batch_to(&mut self, target: CubeRenderTarget) {
        let config = self.fine_config();
        match target {
            CubeRenderTarget::Main => FinePipeline::render(
                &self.client,
                &self.scene,
                &self.scan,
                &self.coarse,
                self.draw_brushes.resources(),
                FineOutputBuffers {
                    target: &mut self.target,
                    clip_spills: &mut self.fine_clip_spills,
                    group_spills: &mut self.fine_group_spills,
                },
                config,
            ),
            CubeRenderTarget::Scratch(ix) => FinePipeline::render(
                &self.client,
                &self.scene,
                &self.scan,
                &self.coarse,
                self.draw_brushes.resources(),
                FineOutputBuffers {
                    target: &mut self.scratch[ix],
                    clip_spills: &mut self.fine_clip_spills,
                    group_spills: &mut self.fine_group_spills,
                },
                config,
            ),
        }
    }

    fn fine_config(&self) -> FineRenderConfig {
        FineRenderConfig {
            lengths: self.lengths,
            size: self.size,
            max_clip_depth: self.max_clip_depth,
            max_group_depth: self.max_group_depth,
        }
    }

    pub fn render(&mut self, scene: &Scene) {
        <Self as Render>::render(self, scene);
    }

    /// Renders text draws using the same [`TextContext`] that created their
    /// [`TextLayout`](crate::TextLayout). The CubeCL backend uploads the
    /// context's cached glyph images as a GPU atlas before coarse/fine run.
    pub fn render_with_text(&mut self, scene: &Scene, text_context: &mut TextContext) {
        self.prepare_scene_with_text(scene, text_context);
        <Self as Render>::scan(self, scene, ());
        <Self as Render>::cumsum(self, scene, ());
        self.clear_target();
        self.execute_prepared_plan(scene);
    }

    /// Renders a scene and returns backend-neutral debug data without writing files.
    pub fn render_with_options(
        &mut self,
        scene: &Scene,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.render(scene);
        let image = self.image();
        let (tile_segment_ranges, segments, backdrops) = self.read_debug_scan_buffers();
        capture_render_debug(
            "cubecl",
            scene,
            &image,
            DebugScanBuffers {
                backdrops: &backdrops,
                tile_segment_ranges: &tile_segment_ranges,
                segments: &segments,
            },
            options,
        )
    }

    fn read_debug_scan_buffers(&self) -> (Vec<TileSegmentRange>, Vec<LineSegment>, Vec<i32>) {
        let starts = self.scan.tile_segment_range_starts.read(&self.client);
        let ends = self.scan.tile_segment_range_ends.read(&self.client);
        let tile_segment_ranges = starts
            .into_iter()
            .zip(ends)
            .map(|(start, end)| TileSegmentRange { start, end })
            .collect();

        let p0x = self.scan.segment_p0x.read(&self.client);
        let p0y = self.scan.segment_p0y.read(&self.client);
        let p1x = self.scan.segment_p1x.read(&self.client);
        let p1y = self.scan.segment_p1y.read(&self.client);
        let y_edge = self.scan.segment_y_edge.read(&self.client);
        let segments = p0x
            .into_iter()
            .zip(p0y)
            .zip(p1x)
            .zip(p1y)
            .zip(y_edge)
            .map(|((((p0x, p0y), p1x), p1y), y_edge)| LineSegment {
                point0: (p0x, p0y),
                point1: (p1x, p1y),
                y_edge,
            })
            .collect();

        let backdrops = self.scan.backdrops.read(&self.client);
        (tile_segment_ranges, segments, backdrops)
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn prepare_scene_for_bench(&mut self, scene: &Scene) {
        self.prepare_scene(scene);
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn run_prepared_stage_for_bench(&mut self, scene: &Scene, stage: CubePreparedStage) {
        let draw_end = self.lengths.draw_count as u32;
        match stage {
            CubePreparedStage::Scan => <Self as Render>::scan(self, scene, ()),
            CubePreparedStage::Cumsum => <Self as Render>::cumsum(self, scene, ()),
            CubePreparedStage::Coarse => <Self as Render>::coarse(
                self,
                scene,
                CoarseBatch {
                    draw_start: 0,
                    draw_end,
                    layer_stack_start: 0,
                    layer_stack_end: 0,
                },
            ),
            CubePreparedStage::Fine => self.fine_batch_to(CubeRenderTarget::Main),
        }
    }

    #[cfg(feature = "profile")]
    /// Starts collecting stage timings for subsequent renderer work.
    ///
    /// The profiler synchronizes the GPU before starting so earlier queued work
    /// is not charged to this profile. Call [`Self::end_profile`] after the
    /// render or prepared-stage sequence you want to measure.
    pub fn start_profile(&mut self) {
        sync_client(&self.client);
        self.profiler.start();
    }

    #[cfg(feature = "profile")]
    /// Stops profiling, synchronizes pending GPU work, and returns timings plus memory usage.
    pub fn end_profile(&mut self) -> &RenderProfile {
        sync_client(&self.client);
        self.profiler.end();
        let memory_entries = self.memory_profile_entries();
        self.profiler.set_memory_entries(memory_entries);
        self.profiler.profile()
    }

    #[cfg(feature = "profile")]
    /// Returns the most recently completed or currently active profile.
    pub fn profile(&self) -> &RenderProfile {
        self.profiler.profile()
    }

    #[cfg(feature = "profile")]
    fn memory_profile_entries(&self) -> Vec<RenderProfileMemoryEntry> {
        let mut entries = Vec::with_capacity(17);
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "config",
            self.config.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "scene",
            self.scene.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "scan",
            self.scan.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "coarse",
            self.coarse.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "draw_brushes",
            self.draw_brushes.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "filter_brushes",
            self.filter_brushes.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "filter_convolves",
            self.filter_convolves.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "filter_paths",
            self.filter_paths.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "filter_transfers",
            self.filter_transfers.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "filter_turbulence",
            self.filter_turbulence.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "target",
            self.target.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "fine_stack_spills",
            MemoryUsage::sum([
                self.fine_clip_spills.memory_usage(),
                self.fine_group_spills.memory_usage(),
            ]),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "scratch",
            MemoryUsage::sum(self.scratch.iter().map(|buffer| buffer.memory_usage())),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Gpu,
            "surface_sources",
            MemoryUsage::sum(
                self.surface_sources
                    .iter()
                    .map(|buffer| buffer.memory_usage()),
            ),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Cpu,
            "scene_upload",
            self.scene_upload.memory_usage(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Cpu,
            "exec_plan",
            self.plan
                .as_ref()
                .map(ExecPlan::memory_usage)
                .unwrap_or_default(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Cpu,
            "prepared_text",
            self.text_data
                .as_ref()
                .map(PreparedTextData::memory_usage)
                .unwrap_or_default(),
        );
        Self::push_memory_entry(
            &mut entries,
            RenderProfileMemorySpace::Cpu,
            "renderer_vectors",
            MemoryUsage::sum([
                MemoryUsage::vec(&self.scratch),
                MemoryUsage::vec(&self.scratch_in_use),
                MemoryUsage::vec(&self.surface_sources),
            ]),
        );
        entries
    }

    #[cfg(feature = "profile")]
    fn push_memory_entry(
        entries: &mut Vec<RenderProfileMemoryEntry>,
        space: RenderProfileMemorySpace,
        name: &'static str,
        usage: MemoryUsage,
    ) {
        entries.push(RenderProfileMemoryEntry {
            space,
            name,
            used_bytes: usage.used_bytes,
            allocated_bytes: usage.allocated_bytes,
        });
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        self.target
            .resize_uninit(&self.client, width as usize * height as usize);
        for scratch in &mut self.scratch {
            scratch.resize_uninit(&self.client, width as usize * height as usize);
        }
    }

    fn prepare_scratch_buffers(&mut self, scratch_count: usize) {
        let pixel_count = self.size.0 as usize * self.size.1 as usize;
        while self.scratch.len() < scratch_count {
            self.scratch
                .push(CubeBuffer::new(&self.client, pixel_count));
        }
        for scratch in &mut self.scratch {
            scratch.resize_uninit(&self.client, pixel_count);
        }
        self.scratch_in_use.clear();
        self.scratch_in_use.resize(self.scratch.len(), false);
    }

    fn prepare_fine_stack_spills(
        &mut self,
        lengths: CubeBufferLengths,
        max_clip_depth: usize,
        max_group_depth: usize,
    ) {
        // Fine keeps the common shallow stack in per-lane registers. These
        // buffers preserve unbounded scene semantics for unusually deep layer
        // nesting without paying shared-memory cost in the common case.
        let lane_count =
            lengths.tile_count * crate::cubecl::pipelines::fine::FINE_WORKGROUP_SIZE as usize;
        let clip_spill_depth = max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH);
        let group_spill_depth = max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH);
        self.fine_clip_spills
            .resize_uninit(&self.client, lane_count * clip_spill_depth);
        self.fine_group_spills.resize_uninit(
            &self.client,
            lane_count * group_spill_depth * FINE_GROUP_SPILL_FIELDS,
        );
    }

    /// Reads the rendered target back into a CPU image.
    ///
    /// Rendering stays GPU-resident; this is the explicit readback point used by
    /// examples and consumers that need pixels on the CPU side.
    pub fn image(&self) -> Image {
        Image {
            width: self.size.0,
            height: self.size.1,
            pixels: self.target.read(&self.client),
        }
    }

    #[cfg(any(test, feature = "bench-api"))]
    fn client(&self) -> &::cubecl::client::ComputeClient<R> {
        &self.client
    }

    #[cfg(feature = "bench-api")]
    #[doc(hidden)]
    pub fn client_for_bench(&self) -> &::cubecl::client::ComputeClient<R> {
        self.client()
    }
}

impl WgpuRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
    }

    #[cfg(feature = "wgpu")]
    pub fn target_rgba8_byte_len(&self) -> wgpu::BufferAddress {
        self.size.0 as wgpu::BufferAddress
            * self.size.1 as wgpu::BufferAddress
            * std::mem::size_of::<u32>() as wgpu::BufferAddress
    }

    #[cfg(feature = "wgpu")]
    /// Copies the rendered target into a caller-owned wgpu texture without CPU readback.
    ///
    /// This is the GPU-resident output path for integrations that already own a
    /// wgpu texture. The destination texture must be a single-sample 2D
    /// `Rgba8Unorm` or `Rgba8UnormSrgb` texture created from the same wgpu
    /// device/queue used to initialize this renderer, and must include
    /// `wgpu::TextureUsages::COPY_DST`.
    pub fn blit_target_to_wgpu_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::Texture,
    ) -> Result<(), WgpuTextureBlitError> {
        self.validate_wgpu_texture_destination(dst)?;
        let copy_size = self.target_rgba8_byte_len();
        if copy_size == 0 {
            return Ok(());
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tileink CubeCL target texture blit"),
        });
        let source = self
            .client
            .get_resource(self.target.handle())
            .map_err(WgpuTextureBlitError::SourceUnavailable)?;
        let source = source.resource();
        if source.size < copy_size {
            return Err(WgpuTextureBlitError::SourceTooSmall {
                required: copy_size,
                available: source.size,
            });
        }
        let source = self.texture_source_buffer(device, &mut encoder, source);
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &source.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: source.offset,
                    bytes_per_row: source.bytes_per_row,
                    rows_per_image: None,
                },
            },
            dst.as_image_copy(),
            self.target_texture_extent(),
        );
        queue.submit([encoder.finish()]);
        Ok(())
    }

    #[cfg(feature = "wgpu")]
    /// Renders the scene, then blits the premultiplied RGBA8 target into `dst`.
    pub fn render_to_wgpu_texture(
        &mut self,
        scene: &Scene,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::Texture,
    ) -> Result<(), WgpuTextureBlitError> {
        self.render(scene);
        self.blit_target_to_wgpu_texture(device, queue, dst)
    }

    #[cfg(feature = "wgpu")]
    fn validate_wgpu_texture_destination(
        &self,
        dst: &wgpu::Texture,
    ) -> Result<(), WgpuTextureBlitError> {
        if dst.width() < self.size.0 || dst.height() < self.size.1 {
            return Err(WgpuTextureBlitError::DestinationTooSmall {
                required_width: self.size.0,
                required_height: self.size.1,
                actual_width: dst.width(),
                actual_height: dst.height(),
            });
        }
        if !dst.usage().contains(wgpu::TextureUsages::COPY_DST) {
            return Err(WgpuTextureBlitError::DestinationUsageMissing(dst.usage()));
        }
        if !matches!(
            dst.format(),
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        ) || dst.dimension() != wgpu::TextureDimension::D2
            || dst.sample_count() != 1
        {
            return Err(WgpuTextureBlitError::UnsupportedDestination {
                format: dst.format(),
                dimension: dst.dimension(),
                sample_count: dst.sample_count(),
            });
        }
        Ok(())
    }

    #[cfg(feature = "wgpu")]
    fn texture_source_buffer(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &::cubecl::wgpu::WgpuResource,
    ) -> WgpuTextureCopySource {
        let row_bytes = self.target_row_bytes();
        let row_alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as wgpu::BufferAddress;
        let height = self.size.1 as wgpu::BufferAddress;
        if self.size.1 <= 1 || row_bytes.is_multiple_of(row_alignment) {
            return WgpuTextureCopySource {
                buffer: source.buffer.clone(),
                offset: source.offset,
                bytes_per_row: (self.size.1 > 1).then_some(row_bytes as u32),
            };
        }

        let padded_row_bytes = align_to(row_bytes, row_alignment);
        let padded = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tileink padded CubeCL target texture blit"),
            size: padded_row_bytes * height,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for row in 0..height {
            encoder.copy_buffer_to_buffer(
                &source.buffer,
                source.offset + row * row_bytes,
                &padded,
                row * padded_row_bytes,
                row_bytes,
            );
        }
        WgpuTextureCopySource {
            buffer: padded,
            offset: 0,
            bytes_per_row: Some(padded_row_bytes as u32),
        }
    }

    #[cfg(feature = "wgpu")]
    fn target_row_bytes(&self) -> wgpu::BufferAddress {
        self.size.0 as wgpu::BufferAddress * std::mem::size_of::<u32>() as wgpu::BufferAddress
    }

    #[cfg(feature = "wgpu")]
    fn target_texture_extent(&self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: self.size.0,
            height: self.size.1,
            depth_or_array_layers: 1,
        }
    }
}

#[cfg(feature = "wgpu")]
struct WgpuTextureCopySource {
    buffer: wgpu::Buffer,
    offset: wgpu::BufferAddress,
    bytes_per_row: Option<u32>,
}

#[cfg(feature = "wgpu")]
fn align_to(value: wgpu::BufferAddress, alignment: wgpu::BufferAddress) -> wgpu::BufferAddress {
    value.next_multiple_of(alignment)
}

#[cfg(feature = "cuda")]
impl CudaRenderer {
    pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self {
        Self::new(&Default::default(), width, height, clear)
    }
}

#[cfg(test)]
mod tests;
