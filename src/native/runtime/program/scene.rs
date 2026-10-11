//! Native GPU scene assembly from CPU plans.
use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use super::scene_scan::{self, ScanOutput};
use crate::{
    Canvas,
    render::{fine::FineParams, prepare::ScenePreparation, upload::scene::SceneUploadStaging},
    shared::{
        execution::ExecPlan, gpu_plan::GpuBufferLengths, image_resource::GpuImageResourceUpload,
    },
    text::PreparedTextData,
};
use std::rc::Rc;

#[derive(Default)]
struct SceneBuffers {
    work: super::cached_buffer::CachedBuffer,
    clip_slot_snapshot: Option<Vec<u8>>,
    tile_bin_layout: Option<(usize, usize, usize, usize)>,
    tile_bin_snapshot: Option<(Vec<u8>, Vec<u8>)>,
    chunks: super::cached_buffer::CachedBuffer,
    spills: super::cached_buffer::CachedBuffer,
    coarse_text: super::cached_buffer::CachedBuffer,
    fine_text: super::cached_buffer::CachedBuffer,
    scan: scene_scan::ScanBuffers,
    draws: super::cached_buffer::CachedBuffer,
    paint: super::cached_buffer::CachedBuffer,
    layers: super::cached_buffer::CachedBuffer,
    batches: super::cached_buffer::CachedBuffer,
}

#[derive(Default)]
pub(crate) struct SceneCache {
    buffers: SceneBuffers,
    pending: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    staging: SceneUploadStaging,
    preparation: ScenePreparation,
    plan: Option<Rc<ExecPlan>>,
}

pub(crate) struct SceneOptions<'a> {
    pub limit: u32,
    pub active: Option<&'a crate::render::damage_tiles::DamageTiles>,
}

/// Keeps compiled metadata paired with its Canvas and cache until scan consumes it.
/// Dropping preparation leaves no uploaded plan falsely marked as reusable.
pub(crate) struct PreparedScene<'a> {
    cache: &'a mut SceneCache,
    canvas: &'a Canvas,
    plan: crate::render::prepare::PreparedPlan,
}

pub(crate) struct Scene {
    pub(crate) active_batches: Vec<u32>,
    // Share the prepared spatial index without copying it each frame. A later
    // preparation uses copy-on-write if an older recorded scene is still alive.
    tile_draw_bins: Rc<crate::shared::gpu_plan::TileDrawBins>,
    draw_count: u32,
    plan: Rc<ExecPlan>,
    layer_count: u32,
    lengths: GpuBufferLengths,
    scan: ScanOutput,
    draws: ResourceId,
    paint: ResourceId,
    coarse_text: ResourceId,
    fine_text: ResourceId,
    layers: ResourceId,
    batches: ResourceId,
    work: ResourceId,
    chunks: ResourceId,
    spills: ResourceId,
    fine_params: FineParams,
    clip_dispatch: clip_tiles::ClipDispatch,
    program_images: Option<super::super::program_shader::BrushImages>,
}

/// These texture handles must come from the upload whose placements patched the
/// scene paint records. Their ownership/descriptor ABI is checked when binding.
pub(crate) struct SceneImages {
    pub(crate) atlas: ResourceId,
    pub(crate) table: ResourceId,
    pub(crate) sampler: ResourceId,
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod upload_journal_tests;

#[cfg(test)]
#[path = "../tests/scene.rs"]
mod tests;

#[path = "scene/images.rs"]
mod images;

#[path = "scene/layers.rs"]
mod layers;

#[path = "scene/vector_images.rs"]
mod vector_images;

#[path = "scene/clip_tiles.rs"]
mod clip_tiles;

#[path = "scene/cache.rs"]
mod cache;
#[path = "scene/dispatch.rs"]
mod dispatch;
#[path = "scene/upload.rs"]
mod upload;
#[path = "scene/work.rs"]
mod work;

#[cfg(test)]
use crate::shared::gpu_coarse::TileDrawRecord;
#[cfg(test)]
use work::tile_bin_dirty_updates;

#[cfg(test)]
#[path = "../tests/scene_filter_geometry.rs"]
mod filter_geometry_tests;
