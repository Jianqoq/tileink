//! Host expectations for compiled shader interfaces; never injected into HLSL.
#[path = "interfaces/coarse.rs"]
mod coarse;
#[path = "interfaces/filter.rs"]
mod filter;
#[path = "interfaces/fine.rs"]
mod fine;
#[path = "interfaces/progressive.rs"]
mod progressive;
#[path = "interfaces/scan.rs"]
mod scan;
#[path = "interfaces/texture.rs"]
mod texture;
#[path = "interfaces/validation.rs"]
mod validation;
use super::abi::{Field, Interface, Kind, Resource};
use crate::gpu_constants as constants;
use std::{io, sync::OnceLock};
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
const fn buffer(binding: u32, kind: Kind) -> Resource {
    Resource {
        count: 1,
        binding,
        kind,
        size: 4,
        fields: Vec::new(),
        internal: false,
    }
}
fn sampler(binding: u32) -> Resource {
    Resource {
        size: 0,
        ..buffer(binding, Kind::Sampler)
    }
}

// Production entries share the descriptor contract directly. Constructing a
// validation interface just to extract this resource wastes unrelated layouts.
#[allow(
    clippy::expect_used,
    reason = "Built-in ABI recipes provide the required entry, resource, field and constant before specialization"
)]
fn texture_table() -> Resource {
    static CAPACITY: OnceLock<u32> = OnceLock::new();
    let count = *CAPACITY.get_or_init(|| {
        constants::parse(include_str!(
            "../../src/shaders/hlsl/shared/texture_table_constants.hlsli"
        ))
        .expect("texture table constants")["NATIVE_TEXTURE_TABLE_CAPACITY"]
    });
    Resource {
        count,
        ..buffer(30, Kind::TextureTable)
    }
}

#[allow(
    clippy::expect_used,
    reason = "Built-in ABI recipes provide the required entry, resource, field and constant before specialization"
)]
fn uniform(binding: u32, fields: &[(&str, u32, u32)], internal: bool) -> Resource {
    Resource {
        count: 1,
        binding,
        kind: Kind::Uniform,
        size: fields
            .last()
            .map(|(_, offset, lanes)| offset + lanes * 4)
            .expect("built-in uniform has fields"),
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
    let result = match family {
        "scan-apply-chunk-offsets"
        | "scan-chunk-offsets"
        | "scan-clear"
        | "scan-count"
        | "scan-emit"
        | "scan-prefix-chunks" => scan::get(family)?,
        "coarse-particle-counts"
        | "coarse-tile-kinds"
        | "coarse-tile-counts"
        | "coarse-emit-chunks"
        | "coarse-emit"
        | "coarse-count"
        | "coarse-emit-allocation"
        | "coarse-emit-offsets"
        | "coarse-prefix" => coarse::get(family)?,
        "pixel-math" | "geometry-math" | "fill-coverage" | "blend-math" => validation::get(family),
        "filter-progressive" => progressive::get(),
        "filter-basic" => filter::basic(),
        "filter-inputs" => filter::inputs(),
        "filter-morphology" => filter::morphology(),
        "filter-displacement" => filter::displacement(),
        "filter-transfer" => filter::transfer(),
        "filter-convolve" => filter::convolve(),
        "filter-resample" => filter::resample(),
        "filter-blur" => filter::blur(false),
        "filter-lighting" => filter::lighting(),
        "filter-rectangle" => filter::rectangle(),
        "filter-path-mask" => filter::path_mask(),
        "filter-turbulence" => filter::turbulence(),
        "filter-surface" => filter::surface(),
        "filter-layer" => filter::layer(),
        "filter-stack" => filter::stack(),
        "filter-glass" => filter::glass(),
        "filter-brush" => filter::brush(),
        "texture-table-validation" => texture::table(),
        "sdf-coverage" => validation::sdf(),
        "filter-blur-shared" => filter::blur(true),
        "fine-gradient" => fine::gradient(),
        "fine-pattern" => fine::pattern(),
        "fine-main" => fine::main(),
        "fine-text" => fine::text(),
        "fine-brush" => fine::brush(),
        "texture-validation" => texture::validation(),
        "texture-array-validation" => texture::array(),
        "sampler-validation" => texture::sampler(),
        "cumsum" => interface(
            [constants::get("CUMSUM_CHUNK_SIZE"), 1, 1],
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
            [constants::get("RANGE_SCATTER_WORKGROUP_SIZE"), 1, 1],
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
