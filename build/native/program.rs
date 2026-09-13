//! Program catalog and shader source/ABI preparation, independent of compilation.
use super::{abi, source::SourceGraph};
use std::{fs, io, path::Path};

pub const FAMILIES: &[(&str, &str, &str)] = &[
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
    let mut graph = SourceGraph::load(&root.join("src/shaders/hlsl"), source)?;
    for name in graph.files.keys() {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("src/shaders/hlsl").join(name).display()
        );
    }
    if family == "range-scatter" {
        graph.expanded = format!(
            "static const uint RANGE_SCATTER_WORKGROUP_SIZE = {}u;\n{}",
            crate::gpu_constants::RANGE_SCATTER_WORKGROUP_SIZE,
            graph.expanded
        );
    }
    if family == "cumsum" {
        graph.expanded = format!(
            "static const uint CUMSUM_CHUNK_SIZE = {}u;\n{}",
            crate::gpu_constants::CUMSUM_CHUNK_SIZE,
            graph.expanded
        );
    }
    println!("cargo:rerun-if-changed={}", root.join(abi_path).display());
    let abi = fs::read(root.join(abi_path))?;
    let mut description: serde_json::Value = serde_json::from_slice(&abi)?;
    let abi = if family.starts_with("scan-") {
        let name = description["record_abi"]
            .as_str()
            .ok_or_else(|| io::Error::other("missing scan record ABI"))?;
        if !valid_record_name(name) {
            return Err(io::Error::other("record ABI must be a local JSON filename"));
        }
        let path = root.join("src/shaders").join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let records: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let mut prelude = format!(
            "static const uint SCAN_CHUNK_SIZE = {}u;\n",
            crate::gpu_constants::SCAN_CHUNK_SIZE
        );
        for (name, constant) in records
            .as_object()
            .filter(|r| !r.is_empty())
            .ok_or_else(|| io::Error::other("empty scan record layout"))?
        {
            if !valid_constant_name(name) {
                return Err(io::Error::other("invalid scan record layout name"));
            }
            let value = constant
                .as_u64()
                .filter(|v| *v <= u32::MAX as u64)
                .ok_or_else(|| io::Error::other("invalid scan record layout constant"))?;
            prelude.push_str(&format!("static const uint {name} = {value}u;\n"));
        }
        prelude.push_str(&format!(
            "static const uint SCAN_TILE_SIZE = {}u;\n",
            crate::gpu_constants::TILE_SIZE
        ));
        graph.expanded = prelude + &graph.expanded;
        description["record_layouts"] = records;
        if description["workgroup"][0] != crate::gpu_constants::SCAN_CHUNK_SIZE {
            return Err(io::Error::other(
                "scan workgroup differs from shared constant",
            ));
        }
        serde_json::to_vec(&description)?
    } else {
        abi
    };
    abi::validate(&description)?;
    if family == "cumsum" && description["workgroup"][0] != crate::gpu_constants::CUMSUM_CHUNK_SIZE
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

fn valid_record_name(name: &str) -> bool {
    name.ends_with(".json")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        && !name.contains("..")
}

fn valid_constant_name(name: &str) -> bool {
    name.starts_with("SCAN_")
        && name
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}
