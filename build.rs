use std::{
    env, fs,
    path::{Path, PathBuf},
};

const WGPU_SHADER_ENTRIES: [(&str, &str); 9] = [
    ("scan.wgsl", "tileink_wgpu_scan.wgsl"),
    ("cumsum.wgsl", "tileink_wgpu_cumsum.wgsl"),
    ("coarse/count.wgsl", "tileink_wgpu_coarse_count.wgsl"),
    ("coarse/prefix.wgsl", "tileink_wgpu_coarse_prefix.wgsl"),
    ("coarse/emit.wgsl", "tileink_wgpu_coarse_emit.wgsl"),
    ("fine.wgsl", "tileink_wgpu_fine.wgsl"),
    ("filter.wgsl", "tileink_wgpu_filter.wgsl"),
    ("fine_web.wgsl", "tileink_wgpu_fine_web.wgsl"),
    ("filter_web.wgsl", "tileink_wgpu_filter_web.wgsl"),
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let shader_dir = manifest_dir.join("src").join("wgpu").join("shaders");
    emit_rerun_if_changed(&shader_dir);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for (entry, output) in WGPU_SHADER_ENTRIES {
        let source = expand_shader(&shader_dir.join(entry), &mut Vec::new());
        fs::write(out_dir.join(output), source).unwrap();
    }
}

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

fn parse_include(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("#include")?.trim();
    rest.strip_prefix('"')?.strip_suffix('"')
}
