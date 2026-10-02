pub(crate) mod affine;
pub(crate) mod bounds;
pub(crate) mod brush;
pub(crate) mod cpu_time;
pub(crate) mod dense_set;
pub(crate) mod draw_record;
pub(crate) mod execution;
pub(crate) mod fill;
pub(crate) mod gpu_brush;
pub(crate) mod gpu_coarse;
pub(crate) mod gpu_constants;
pub(crate) mod gpu_layout;
pub(crate) mod gpu_plan;
pub(crate) mod gpu_sdf;
pub(crate) mod gpu_text;
pub(crate) mod gpu_types;
pub(crate) mod image;
pub(crate) mod image_resource;
pub(crate) mod layer;
pub(crate) mod line;
pub(crate) mod line_seg;
pub(crate) mod offscreen;
pub(crate) mod path;
pub(crate) mod path_flatten;
pub(crate) mod pixel;
pub(crate) mod scan_line;
pub(crate) mod scene_arena;
pub(crate) mod sdf;
pub(crate) mod tile_seg_range;

pub(crate) mod fine_config;
// SAFETY: repr(C) and twenty contiguous u32 fields contain no padding.
unsafe impl bytemuck::Zeroable for fine_config::FineConfig {}
unsafe impl bytemuck::Pod for fine_config::FineConfig {}

pub(crate) mod filter_config;
pub(crate) mod filter_parameters;
pub(crate) mod progressive_blur_config;
// SAFETY: repr(C), contiguous four-byte scalars and five four-float vectors, with no padding.
unsafe impl bytemuck::Zeroable for filter_config::FilterConfig {}
unsafe impl bytemuck::Pod for filter_config::FilterConfig {}
