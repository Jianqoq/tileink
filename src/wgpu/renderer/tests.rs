mod backdrop_history;
mod backdrop_resize;
mod common;
mod filters_color;
mod filters_effects;
mod filters_graph;
mod filters_graph_inputs;
mod filters_layers;
mod filters_masks;
mod gpu_paths;
mod image_atlas;
mod pipelines;
mod portable_history;
mod profiles;
mod resources;
mod retained_reorder;
mod retained_structure;
mod retained_updates;
mod submissions;
mod target_capacity;
mod targets;
mod text;

use common::*;
use resources::render_resource_atlas_test;

const GPU_PTCL_END: u32 = 0;
const GPU_PTCL_COLOR: u32 = 2;
const GPU_PTCL_END_CLIP: u32 = 4;
const GPU_PTCL_SDF: u32 = 9;
const GPU_PTCL_BEGIN_SDF_CLIP: u32 = 12;
const GPU_PTCL_IMAGE: u32 = 13;

struct ForceCoarseChunksGuard {
    previous: bool,
}

impl ForceCoarseChunksGuard {
    fn new() -> Self {
        Self {
            previous: force_coarse_emit_chunks_for_test(true),
        }
    }
}

impl Drop for ForceCoarseChunksGuard {
    fn drop(&mut self) {
        force_coarse_emit_chunks_for_test(self.previous);
    }
}

mod retained_removal;

mod vector_images;

mod empty_layers;

mod filter_scheduling;

mod backdrop_scheduling;

mod backdrop_cache;

mod filter_dependencies;

mod backdrop_damage;

mod backdrop_order;

mod clip_reprepare;
mod filter_sampling;
