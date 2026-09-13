//! Program catalog and shader source/ABI preparation, independent of compilation.
use super::{abi, source::SourceGraph};
use std::{fs, io, path::Path};

pub const FAMILIES: &[(&str, &str, &str)] = &[
    (
        "coarse-emit-allocation",
        "coarse/emit_allocation.hlsl",
        "src/shaders/coarse-emit-allocation-abi.json",
    ),
    (
        "coarse-emit-offsets",
        "coarse/emit_allocation.hlsl",
        "src/shaders/coarse-emit-offsets-abi.json",
    ),
    (
        "coarse-prefix",
        "coarse/prefix.hlsl",
        "src/shaders/coarse-prefix-abi.json",
    ),
    ("probe", "probes.hlsl", "src/shaders/probe-abi.json"),
    (
        "range-scatter",
        "range_scatter.hlsl",
        "src/shaders/range-scatter-abi.json",
    ),
    ("cumsum", "cumsum.hlsl", "src/shaders/cumsum-abi.json"),
    (
        "scan-prefix_chunks",
        "scan/prefix_chunks.hlsl",
        "src/shaders/scan-prefix-chunks-abi.json",
    ),
    (
        "scan-chunk_offsets",
        "scan/chunk_offsets.hlsl",
        "src/shaders/scan-chunk-offsets-abi.json",
    ),
    (
        "scan-apply_chunk_offsets",
        "scan/apply_chunk_offsets.hlsl",
        "src/shaders/scan-apply-chunk-offsets-abi.json",
    ),
    (
        "scan-clear",
        "scan/clear.hlsl",
        "src/shaders/scan-clear-abi.json",
    ),
    (
        "scan-count",
        "scan/count.hlsl",
        "src/shaders/scan-count-abi.json",
    ),
    (
        "scan-emit",
        "scan/emit.hlsl",
        "src/shaders/scan-emit-abi.json",
    ),
];

pub struct Prepared {
    pub graph: SourceGraph,
    pub description: serde_json::Value,
    pub abi: Vec<u8>,
}

pub fn prepare(root: &Path, family: &str, source: &str, abi_path: &str) -> io::Result<Prepared> {
    let graph = SourceGraph::load(&root.join("src/shaders/hlsl"), source)?;
    for name in graph.files.keys() {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("src/shaders/hlsl").join(name).display()
        );
    }
    println!("cargo:rerun-if-changed={}", root.join(abi_path).display());
    let abi = fs::read(root.join(abi_path))?;
    let description: serde_json::Value = serde_json::from_slice(&abi)?;
    abi::validate(&description)?;
    if family == "cumsum"
        && description["workgroup"][0] != crate::gpu_constants::get("CUMSUM_CHUNK_SIZE")
    {
        return Err(io::Error::other(
            "cumsum ABI workgroup differs from shared algorithm constant",
        ));
    }
    Ok(Prepared {
        graph,
        description,
        abi,
    })
}
