use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, SamplerFilter},
};
use crate::shared::gpu_coarse::{PtclRecord, TileCoarseRecord};
use crate::shared::{
    draw_record::DrawRecord,
    fine_config::FineConfig,
    gpu_constants::{NATIVE_TEXTURE_TABLE_CAPACITY, TILE_SIZE},
    line_seg::LineSegment,
};

pub(super) struct FineFixture {
    pub config: FineConfig,
    pub draws: Vec<DrawRecord>,
    pub paint: Vec<u32>,
    pub text: Vec<u32>,
    pub coarse: Vec<u32>,
    pub segments: Vec<LineSegment>,
    pub spills: Vec<u32>,
    pub image: ([u32; 2], Vec<u8>),
}
impl FineFixture {
    pub fn tile(particles: &[PtclRecord]) -> Self {
        let tile = TileCoarseRecord {
            ptcl_count: particles.len() as u32,
            ptcl_end: particles.len() as u32,
            ..Default::default()
        };
        let mut coarse = bytemuck::cast_slice::<_, u32>(&[tile]).to_vec();
        coarse.extend(bytemuck::cast_slice::<_, u32>(particles));
        // Reserve one glyph-list index before the tile kind; other fixtures do
        // not consume it, and glyph fixtures use the real packed coarse layout.
        coarse.push(0);
        let kind = coarse.len() as u32;
        coarse.push(0);
        Self {
            config: FineConfig {
                width: TILE_SIZE,
                height: TILE_SIZE,
                tiles_width: 1,
                tiles_height: 1,
                tile_count: 1,
                ptcl_capacity: particles.len() as u32,
                active_tile_count: 1,
                dispatch_width: 1,
                fine_tile_kind_base: kind,
                ..Default::default()
            },
            draws: Vec::new(),
            paint: vec![0],
            text: vec![0],
            coarse,
            segments: vec![LineSegment::default()],
            spills: vec![0x37373737; 7],
            image: ([1, 1], vec![0; 4]),
        }
    }
    pub fn batch(&self) -> Result<ComputeBatch> {
        let mut batch = ComputeBatch::new();
        let config = batch.buffer(bytemuck::bytes_of(&self.config).to_vec())?;
        let target = batch.texture_rgba8(
            [self.config.width, self.config.height],
            self.config
                .clear_color
                .to_le_bytes()
                .repeat((self.config.width * self.config.height) as usize),
        )?;
        let draws = batch.buffer(if self.draws.is_empty() {
            vec![0; std::mem::size_of::<DrawRecord>()]
        } else {
            bytemuck::cast_slice(&self.draws).to_vec()
        })?;
        let paint = batch.buffer(bytemuck::cast_slice(&self.paint).to_vec())?;
        let coarse = batch.buffer(bytemuck::cast_slice(&self.coarse).to_vec())?;
        let segments = batch.buffer(bytemuck::cast_slice(&self.segments).to_vec())?;
        let text = batch.buffer(bytemuck::cast_slice(&self.text).to_vec())?;
        let spills = batch.buffer(bytemuck::cast_slice(&self.spills).to_vec())?;
        let (size, pixels) = &self.image;
        let atlas = batch.texture_array_rgba8([size[0], size[1], 1], pixels.clone())?;
        let image = batch.texture_rgba8(*size, pixels.clone())?;
        let images = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
        let sampler = batch.sampler(SamplerFilter::Linear)?;
        // SAFETY: callers construct complete fixture records and spill capacity;
        // their input domains are fixed and separately asserted in the tests.
        unsafe {
            batch.dispatch(
                "fine_tile_main",
                &[
                    (0, config),
                    (1, target),
                    (2, draws),
                    (3, paint),
                    (4, coarse),
                    (5, segments),
                    (6, text),
                    (7, spills),
                    (12, atlas),
                    (13, sampler),
                    (30, images),
                ],
                [
                    self.config.dispatch_width,
                    self.config
                        .active_tile_count
                        .div_ceil(self.config.dispatch_width)
                        + 1,
                    1,
                ],
            )?;
        }
        batch.readback(target)?;
        batch.readback(spills)?;
        Ok(batch)
    }
}
pub(super) fn routes() -> Result<super::four_api::Routes> {
    super::four_api::Routes::with_features(
        wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    )
}
pub(super) fn draw(color: u32) -> Result<(DrawRecord, Vec<u32>)> {
    use crate::shared::{affine::GpuAffine, bounds::PixelBounds};
    use bytemuck::Zeroable;
    let mut record = DrawRecord::zeroed();
    record.path_id = DrawRecord::NONE;
    record.glyph_run_id = DrawRecord::NONE;
    record.sdf_offset = DrawRecord::NONE;
    record.sdf_shadow_offset = DrawRecord::NONE;
    record.transform = GpuAffine::IDENTITY;
    record.inverse_transform = GpuAffine::IDENTITY;
    record.pixel_bounds = PixelBounds {
        x0: 0,
        y0: 0,
        x1: TILE_SIZE as i32,
        y1: TILE_SIZE as i32,
    };
    record.local_pixel_bounds = record.pixel_bounds;
    let constants = super::hlsl_constants::read_hlsl("shared/brush/constants.hlsli")?;
    let mut paint =
        vec![0; (constants["BRUSH_HEADER_WORDS"] + constants["BRUSH_PARAM_WORDS"]) as usize];
    paint[0] = constants["BRUSH_SOLID"];
    paint[constants["BRUSH_COLOR_WORD"] as usize] = color;
    record.brush_len = paint.len() as u32;
    Ok((record, paint))
}
