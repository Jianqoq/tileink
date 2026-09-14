#[cfg(feature = "wgpu")]
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[cfg(feature = "wgpu")]
#[path = "build/dxc.rs"]
mod dxc;
#[cfg(feature = "wgpu")]
#[path = "build/dxil.rs"]
mod dxil;
#[cfg(feature = "wgpu")]
#[path = "build/dxil_cache.rs"]
mod dxil_cache;
#[cfg(feature = "wgpu")]
#[path = "src/wgpu/dxil_manifest.rs"]
mod dxil_manifest;
#[cfg(feature = "wgpu")]
#[path = "build/dxil_provenance.rs"]
mod dxil_provenance;
#[cfg(feature = "wgpu")]
#[path = "src/wgpu/shader_variants.rs"]
mod shader_variants;

#[cfg(feature = "wgpu")]
const WGPU_SHADER_ENTRIES: [(&str, &str); 16] = [
    ("scan/clear.wgsl", "tileink_wgpu_scan_clear.wgsl"),
    ("scan/count.wgsl", "tileink_wgpu_scan_count.wgsl"),
    (
        "scan/prefix_chunks.wgsl",
        "tileink_wgpu_scan_prefix_chunks.wgsl",
    ),
    (
        "scan/chunk_offsets.wgsl",
        "tileink_wgpu_scan_chunk_offsets.wgsl",
    ),
    (
        "scan/apply_chunk_offsets.wgsl",
        "tileink_wgpu_scan_apply_chunk_offsets.wgsl",
    ),
    ("scan/emit.wgsl", "tileink_wgpu_scan_emit.wgsl"),
    ("cumsum.wgsl", "tileink_wgpu_cumsum.wgsl"),
    ("range_scatter.wgsl", "tileink_wgpu_range_scatter.wgsl"),
    ("coarse/count.wgsl", "tileink_wgpu_coarse_count.wgsl"),
    ("coarse/prefix.wgsl", "tileink_wgpu_coarse_prefix.wgsl"),
    ("coarse/emit.wgsl", "tileink_wgpu_coarse_emit.wgsl"),
    ("coarse/emit_web.wgsl", "tileink_wgpu_coarse_emit_web.wgsl"),
    ("fine.wgsl", "tileink_wgpu_fine.wgsl"),
    ("filter.wgsl", "tileink_wgpu_filter.wgsl"),
    ("fine_web.wgsl", "tileink_wgpu_fine_web.wgsl"),
    ("filter_web.wgsl", "tileink_wgpu_filter_web.wgsl"),
];

#[cfg(feature = "wgpu")]
fn build_wgpu() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    for input in [
        "build.rs",
        "build/dxc.rs",
        "build/dxil.rs",
        "build/dxil_provenance.rs",
        "src/wgpu/dxil_manifest.rs",
        "src/wgpu/shader_variants.rs",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_dir.join(input).display()
        );
    }
    let shader_dir = manifest_dir.join("src").join("wgpu").join("shaders");
    emit_rerun_if_changed(&shader_dir);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut fine_portable_source = None;
    for (entry, output) in WGPU_SHADER_ENTRIES {
        let mut source = expand_shader(&shader_dir.join(entry), &mut Vec::new());
        if entry == "range_scatter.wgsl" {
            source = format!(
                "const RANGE_SCATTER_WORKGROUP_SIZE: u32 = {}u;\n{source}",
                gpu_constants::get("RANGE_SCATTER_WORKGROUP_SIZE")
            );
        }
        if entry == "cumsum.wgsl" {
            source = format!(
                "const CUMSUM_CHUNK_SIZE: u32 = {}u;\n{source}",
                gpu_constants::get("CUMSUM_CHUNK_SIZE")
            );
        }
        if entry.starts_with("coarse/") {
            source = format!(
                "const COARSE_WORKGROUP_SIZE: u32 = {}u;\n{source}",
                gpu_constants::get("COARSE_WORKGROUP_SIZE")
            );
        }
        if entry.starts_with("scan/") {
            source = format!(
                "const SCAN_CHUNK_SIZE: u32 = {}u;\nconst SCAN_TILE_SIZE: u32 = {}u;\n{source}",
                gpu_constants::get("SCAN_CHUNK_SIZE"),
                gpu_constants::get("TILE_SIZE")
            );
        }
        if entry.starts_with("fine") {
            source = format!(
                "const FINE_WORKGROUP_SIZE: u32 = {}u;\n{source}",
                gpu_constants::get("FINE_WORKGROUP_SIZE")
            );
        }
        if entry.starts_with("filter") {
            source = format!(
                "const FILTER_WORKGROUP_SIZE: u32 = {}u;\nconst SHARED_BLUR_TILE_WIDTH: u32 = {}u;\nconst SHARED_BLUR_TILE_HEIGHT: u32 = {}u;\nconst SHARED_BLUR_MAX_RADIUS: u32 = {}u;\nconst COMPONENT_TRANSFER_TABLE_SIZE: u32 = {}u;\nconst COMPONENT_TRANSFER_TABLE_LEN: u32 = {}u;\n{source}",
                gpu_constants::get("FILTER_WORKGROUP_SIZE"),
                gpu_constants::get("SHARED_BLUR_TILE_WIDTH"),
                gpu_constants::get("SHARED_BLUR_TILE_HEIGHT"),
                gpu_constants::get("SHARED_BLUR_MAX_RADIUS"),
                gpu_constants::get("COMPONENT_TRANSFER_TABLE_SIZE"),
                gpu_constants::get("COMPONENT_TRANSFER_TABLE_LEN")
            );
        }
        if entry == "fine_web.wgsl" {
            fine_portable_source = Some(source.clone());
        }
        fs::write(out_dir.join(output), source).unwrap();
    }
    dxil::generate(&fine_portable_source.unwrap(), &out_dir);
}

#[cfg(feature = "wgpu")]
fn emit_rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                emit_rerun_if_changed(&path);
            } else {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

#[cfg(feature = "wgpu")]
fn expand_shader(path: &Path, stack: &mut Vec<PathBuf>) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&canonical) {
        panic!("cyclic WGSL include: {}", path.display());
    }
    stack.push(canonical);

    let source = fs::read_to_string(path).unwrap();
    let mut expanded = String::new();
    for line in source.lines() {
        if let Some(include) = parse_include(line) {
            let include_path = path.parent().unwrap().join(include);
            expanded.push_str(&format!("// begin include {}\n", include_path.display()));
            expanded.push_str(&expand_shader(&include_path, stack));
            expanded.push_str(&format!("// end include {}\n", include_path.display()));
        } else {
            expanded.push_str(line);
            expanded.push('\n');
        }
    }

    stack.pop();
    expanded
}

#[cfg(feature = "wgpu")]
fn parse_include(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("#include")?.trim();
    rest.strip_prefix('"')?.strip_suffix('"')
}

#[cfg(any(feature = "native-dx12", feature = "native-vulkan"))]
#[path = "build/native.rs"]
mod native_shaders;

#[path = "build/gpu_constants.rs"]
mod gpu_constants;

fn main() {
    println!("cargo:rerun-if-changed=build/gpu_constants.rs");
    println!("cargo:rerun-if-changed=build/hlsl_source.rs");
    println!("cargo:rerun-if-changed=src/shaders/hlsl/constants.hlsli");
    gpu_constants::write_rust(&std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()))
        .expect("generate host GPU constants");
    #[cfg(any(feature = "native-dx12", feature = "native-vulkan"))]
    native_shaders::generate()
        .unwrap_or_else(|error| panic!("native shader build failed: {error}"));
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "wgpu")]
    build_wgpu();
}
