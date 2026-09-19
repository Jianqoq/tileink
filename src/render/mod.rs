//! Backend-independent preparation, damage decisions, and diagnostics.
//! GPU adapters consume these algorithms without owning scene semantics.
pub(crate) mod damage_tiles;
pub(crate) mod incremental;
pub(crate) mod profile;
pub(crate) mod upload;

pub(crate) mod backend;
pub(crate) mod batches;
pub(crate) mod binning;
pub(crate) mod commands;
pub(crate) mod dispatch;
pub(crate) mod filter_resources;
pub(crate) mod output;
pub(crate) mod resource_writes;
pub(crate) mod retained;
pub(crate) mod retained_surfaces;
pub(crate) mod target_capacity;
pub(crate) mod vector_images;

pub(crate) mod draw_batches;

pub(crate) mod scene_resources;

pub(crate) mod groups;
pub(crate) mod operations;

pub(crate) mod masks;
pub(crate) mod scratch_slots;
pub(crate) mod surfaces;

pub(crate) mod filter_scene;

pub(crate) mod filter_pass;

pub(crate) mod filters;

pub(crate) mod backdrops;

pub(crate) mod frame;
pub(crate) mod layers;

pub(crate) mod prepare;

pub(crate) mod filter_program;

pub(crate) mod coarse;

pub(crate) mod fine;
