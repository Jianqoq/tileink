//! Program catalog and shader source/ABI preparation, independent of compilation.
use super::{abi, source::SourceGraph};
use std::{io, path::Path};

pub const FAMILIES: &[(&str, &str)] = &[
    ("coarse-particle-counts", "coarse/particle_counts.hlsl"),
    ("coarse-tile-kinds", "coarse/tile_kinds.hlsl"),
    ("coarse-tile-counts", "coarse/tile_counts.hlsl"),
    ("coarse-emit-chunks", "coarse/emit_chunks.hlsl"),
    ("coarse-emit", "coarse/emit.hlsl"),
    ("coarse-count", "coarse/count.hlsl"),
    ("coarse-emit-allocation", "coarse/emit_allocation.hlsl"),
    ("coarse-emit-offsets", "coarse/emit_allocation.hlsl"),
    ("coarse-prefix", "coarse/prefix.hlsl"),
    ("filter-basic", "filter/basic.hlsl"),
    ("filter-inputs", "filter/inputs.hlsl"),
    ("filter-morphology", "filter/morphology.hlsl"),
    ("filter-displacement", "filter/displacement.hlsl"),
    ("filter-transfer", "filter/transfer.hlsl"),
    ("filter-convolve", "filter/convolve.hlsl"),
    ("filter-resample", "filter/resample.hlsl"),
    ("fine-gradient", "validation/gradient.hlsl"),
    ("fine-pattern", "validation/pattern.hlsl"),
    ("texture-validation", "validation/texture.hlsl"),
    ("texture-array-validation", "validation/texture_array.hlsl"),
    ("sampler-validation", "validation/sampler.hlsl"),
    ("blend-math", "validation/blend.hlsl"),
    ("fill-coverage", "validation/geometry.hlsl"),
    ("geometry-math", "validation/geometry.hlsl"),
    ("pixel-math", "validation/pixel.hlsl"),
    ("probe", "probes.hlsl"),
    ("range-scatter", "range_scatter.hlsl"),
    ("cumsum", "cumsum.hlsl"),
    ("scan-prefix-chunks", "scan/prefix_chunks.hlsl"),
    ("scan-chunk-offsets", "scan/chunk_offsets.hlsl"),
    ("scan-apply-chunk-offsets", "scan/apply_chunk_offsets.hlsl"),
    ("scan-clear", "scan/clear.hlsl"),
    ("scan-count", "scan/count.hlsl"),
    ("scan-emit", "scan/emit.hlsl"),
];

pub struct Prepared {
    pub graph: SourceGraph,
    pub description: abi::Interface,
    pub abi: Vec<u8>,
}

pub fn prepare(root: &Path, family: &str, source: &str) -> io::Result<Prepared> {
    let graph = SourceGraph::load(&root.join("src/shaders/hlsl"), source)?;
    for name in graph.files.keys() {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("src/shaders/hlsl").join(name).display()
        );
    }
    let description = super::interfaces::get(family)?;
    let abi = description.cache_bytes();
    Ok(Prepared {
        graph,
        description,
        abi,
    })
}
