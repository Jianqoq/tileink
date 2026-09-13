//! Program catalog and shader source/ABI preparation, independent of compilation.
use super::{abi, source::SourceGraph};
use std::{io, path::Path};

pub const FAMILIES: &[(&str, &str)] = &[
    ("coarse-emit-allocation", "coarse/emit_allocation.hlsl"),
    ("coarse-emit-offsets", "coarse/emit_allocation.hlsl"),
    ("coarse-prefix", "coarse/prefix.hlsl"),
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
