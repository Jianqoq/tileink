//! Host expectations for compiled shader interfaces; never injected into HLSL.
#[path = "interfaces/coarse.rs"]
mod coarse;
#[path = "interfaces/filter.rs"]
mod filter;
#[path = "interfaces/fine.rs"]
mod fine;
#[path = "interfaces/scan.rs"]
mod scan;
#[path = "interfaces/texture.rs"]
mod texture;
#[path = "interfaces/validation.rs"]
mod validation;
use super::abi::{Field, Interface, Kind, Resource};
use crate::gpu_constants as constants;
use std::{collections::BTreeMap, io};
const COARSE_CONFIG: &[(&str, u32, u32)] = &[
    ("tile_count", 0, 1),
    ("tiles_width", 4, 1),
    ("tiles_height", 8, 1),
    ("draw_start", 12, 1),
    ("draw_end", 16, 1),
    ("layer_stack_start", 20, 1),
    ("layer_stack_end", 24, 1),
    ("ptcl_capacity", 28, 1),
    ("glyph_capacity", 32, 1),
    ("chunk_count", 36, 1),
    ("text_run_count", 40, 1),
    ("text_glyph_count", 44, 1),
    ("tile_draw_index_count", 48, 1),
    ("emit_chunk_capacity", 52, 1),
    ("paint_brush_base", 56, 1),
    ("text_enabled", 60, 1),
    ("active_tile_count", 64, 1),
    ("active_tile_list_base", 68, 1),
    ("incremental", 72, 1),
];
const CUMSUM_CONFIG: &[(&str, u32, u32)] = &[
    ("row_count", 0, 1),
    ("chunk_count", 4, 1),
    ("_pad0", 8, 1),
    ("_pad1", 12, 1),
];
const DISPATCH_GRID: &[(&str, u32, u32)] =
    &[("x", 0, 1), ("y", 4, 1), ("z", 8, 1), ("_pad", 12, 1)];
const PROBE_PARAMS: &[(&str, u32, u32)] = &[
    ("count", 0, 1),
    ("source_offset", 4, 1),
    ("destination_offset", 8, 1),
    ("stride", 12, 1),
    ("value", 16, 4),
];
const SCAN_CONFIG: &[(&str, u32, u32)] = &[
    ("clear_len", 0, 1),
    ("backdrop_len", 4, 1),
    ("path_count", 8, 1),
    ("scan_chunk_count", 12, 1),
    ("line_count", 16, 1),
    ("segment_capacity", 20, 1),
    ("incremental", 24, 1),
    ("line_base", 28, 1),
    ("path_base", 32, 1),
    ("chunk_base", 36, 1),
    ("backdrop_base", 40, 1),
];
fn buffer(binding: u32, kind: Kind) -> Resource {
    Resource {
        count: 1,
        binding,
        kind,
        size: 4,
        fields: Vec::new(),
        internal: false,
    }
}
fn uniform(binding: u32, fields: &[(&str, u32, u32)], internal: bool) -> Resource {
    Resource {
        count: 1,
        binding,
        kind: Kind::Uniform,
        size: fields
            .last()
            .map_or(0, |(_, offset, lanes)| offset + lanes * 4),
        fields: fields
            .iter()
            .map(|&(name, offset, lanes)| Field {
                name: name.into(),
                offset,
                lanes,
                scalar: super::abi::Scalar::U32,
            })
            .collect(),
        internal,
    }
}
fn interface(
    workgroup: [u32; 3],
    resources: &[(&str, Resource)],
    entries: &[(&str, &[&str])],
) -> Interface {
    Interface {
        workgroup,
        descriptor_set: 0,
        resources: resources
            .iter()
            .map(|(name, r)| ((*name).into(), r.clone()))
            .collect(),
        entries: entries
            .iter()
            .map(|(entry, names)| ((*entry).into(), names.iter().map(|s| (*s).into()).collect()))
            .collect(),
    }
}
pub fn get(family: &str) -> io::Result<Interface> {
    let constants: BTreeMap<String, u32> =
        constants::parse(include_str!("../../src/shaders/hlsl/constants.hlsli"))?;
    let result = match family {
        "scan-apply-chunk-offsets"
        | "scan-chunk-offsets"
        | "scan-clear"
        | "scan-count"
        | "scan-emit"
        | "scan-prefix-chunks" => scan::get(family, &constants)?,
        "coarse-particle-counts"
        | "coarse-tile-kinds"
        | "coarse-tile-counts"
        | "coarse-emit-chunks"
        | "coarse-emit"
        | "coarse-count"
        | "coarse-emit-allocation"
        | "coarse-emit-offsets"
        | "coarse-prefix" => coarse::get(family, &constants)?,
        "pixel-math" | "geometry-math" | "fill-coverage" | "blend-math" => {
            validation::get(family, &constants)
        }
        "filter-basic" => filter::basic(&constants),
        "filter-inputs" => filter::inputs(&constants),
        "filter-morphology" => filter::morphology(&constants),
        "filter-displacement" => filter::displacement(&constants),
        "filter-transfer" => filter::transfer(&constants),
        "filter-convolve" => filter::convolve(&constants),
        "filter-resample" => filter::resample(&constants),
        "filter-blur" => filter::blur(&constants, false),
        "filter-lighting" => filter::lighting(&constants),
        "filter-rectangle" => filter::rectangle(&constants),
        "filter-path-mask" => filter::path_mask(&constants),
        "filter-turbulence" => filter::turbulence(&constants),
        "filter-surface" => filter::surface(&constants),
        "filter-layer" => filter::layer(&constants),
        "filter-stack" => filter::stack(&constants),
        "filter-glass" => filter::glass(&constants),
        "filter-brush" => filter::brush(&constants),
        "texture-table-validation" => texture::table(&constants),
        "sdf-coverage" => validation::sdf(&constants),
        "filter-blur-shared" => filter::blur(&constants, true),
        "fine-gradient" => fine::gradient(&constants),
        "fine-pattern" => fine::pattern(&constants),
        "fine-text" => fine::text(&constants),
        "fine-brush" => fine::brush(&constants),
        "texture-validation" => texture::validation(&constants),
        "texture-array-validation" => texture::array(&constants),
        "sampler-validation" => texture::sampler(&constants),
        "cumsum" => interface(
            [constants["CUMSUM_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, CUMSUM_CONFIG, false)),
                ("chunk_backdrop_offsets", buffer(1, Kind::Read)),
                ("chunk_lens", buffer(2, Kind::Read)),
                ("row_chunk_starts", buffer(3, Kind::Read)),
                ("row_chunk_ends", buffer(4, Kind::Read)),
                ("backdrops", buffer(5, Kind::Write)),
                ("chunk_totals", buffer(6, Kind::Write)),
                ("chunk_offsets", buffer(7, Kind::Write)),
                ("dispatch_grid", uniform(31, DISPATCH_GRID, true)),
            ],
            &[
                (
                    "cumsum_prefix_chunks",
                    &[
                        "config",
                        "chunk_backdrop_offsets",
                        "chunk_lens",
                        "backdrops",
                        "chunk_totals",
                        "dispatch_grid",
                    ],
                ),
                (
                    "cumsum_chunk_offsets",
                    &[
                        "config",
                        "row_chunk_starts",
                        "row_chunk_ends",
                        "chunk_totals",
                        "chunk_offsets",
                    ],
                ),
                (
                    "cumsum_apply_chunk_offsets",
                    &[
                        "config",
                        "chunk_backdrop_offsets",
                        "chunk_lens",
                        "backdrops",
                        "chunk_offsets",
                        "dispatch_grid",
                    ],
                ),
            ],
        ),
        "probe" => interface(
            [64, 1, 1],
            &[
                ("destination", buffer(0, Kind::Write)),
                ("source", buffer(1, Kind::Read)),
                ("params", uniform(2, PROBE_PARAMS, false)),
                ("texels", buffer(3, Kind::Texture)),
            ],
            &[
                ("clear_words", &["destination", "params"]),
                ("copy_words", &["destination", "source", "params"]),
                ("layout_words", &["destination", "params"]),
                (
                    "sample_words",
                    &["destination", "source", "params", "texels"],
                ),
            ],
        ),
        "range-scatter" => interface(
            [constants["RANGE_SCATTER_WORKGROUP_SIZE"], 1, 1],
            &[
                ("destination", buffer(0, Kind::Write)),
                ("source", buffer(1, Kind::Read)),
            ],
            &[("range_scatter", &["destination", "source"])],
        ),
        _ => return Err(io::Error::other(format!("unknown shader family: {family}"))),
    };
    super::abi::validate(&result)?;
    Ok(result)
}
