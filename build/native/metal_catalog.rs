//! MSL programs use the common host ABI catalog with explicit Metal slot mapping.
use super::{
    abi,
    cache::{CacheKey, ShaderCache},
    interfaces,
    metal::{self, MetalCompiler},
    source::SourceGraph,
    write_changed,
};
use std::{env, io, path::Path};

pub fn generate(
    root: &Path,
    out: &Path,
    declarations: &mut String,
    manifest: &mut Vec<serde_json::Value>,
) -> io::Result<()> {
    for variable in [
        "DEVELOPER_DIR",
        "SDKROOT",
        "TILEINK_NATIVE_SHADER_CACHE_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    for file in ["build/native/metal.rs", "build/native/metal_catalog.rs"] {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }
    let compiler = MetalCompiler::discover("/usr/bin/xcrun".into())?;
    let cache = ShaderCache::new(
        env::var_os("TILEINK_NATIVE_SHADER_CACHE_DIR")
            .map(Into::into)
            .unwrap_or_else(|| root.join("target/tileink-metal-shaders")),
    );
    for (family, source) in [
        ("probe", "probes.metal"),
        ("range-scatter", "range_scatter.metal"),
        ("cumsum", "cumsum.metal"),
        ("filter-basic", "filter/basic.metal"),
        ("filter-glass", "filter/glass.metal"),
        ("filter-turbulence", "filter/turbulence.metal"),
        ("filter-lighting", "filter/lighting.metal"),
        ("filter-convolve", "filter/convolve.metal"),
        ("filter-stack", "filter/stack.metal"),
        ("filter-path-mask", "filter/path_mask.metal"),
        ("filter-surface", "filter/surface.metal"),
        ("filter-brush", "filter/brush.metal"),
        ("filter-layer", "filter/layer.metal"),
        ("filter-rectangle", "filter/rectangle.metal"),
        ("filter-blur", "filter/blur.metal"),
        ("filter-blur-shared", "filter/blur_shared.metal"),
        ("filter-resample", "filter/resample.metal"),
        ("filter-displacement", "filter/displacement.metal"),
        ("filter-inputs", "filter/inputs.metal"),
        ("filter-morphology", "filter/morphology.metal"),
        ("filter-transfer", "filter/transfer.metal"),
        ("sdf-coverage", "validation/sdf.metal"),
        ("fine-main", "fine/main.metal"),
        ("scan-clear", "scan/clear.metal"),
        ("scan-count", "scan/count.metal"),
        ("scan-prefix-chunks", "scan/prefix_chunks.metal"),
        ("scan-chunk-offsets", "scan/chunk_offsets.metal"),
        ("scan-apply-chunk-offsets", "scan/apply_chunk_offsets.metal"),
        ("scan-emit", "scan/emit.metal"),
        ("coarse-prefix", "coarse/prefix.metal"),
        ("coarse-count", "coarse/count.metal"),
        ("coarse-emit", "coarse/emit.metal"),
        ("coarse-emit-chunks", "coarse/emit_chunks.metal"),
        ("coarse-particle-counts", "coarse/particle_counts.metal"),
        ("coarse-tile-counts", "coarse/tile_counts.metal"),
        ("coarse-tile-kinds", "coarse/tile_kinds.metal"),
        ("coarse-emit-allocation", "coarse/emit_allocation.metal"),
        ("coarse-emit-offsets", "coarse/emit_allocation.metal"),
    ] {
        let graph = SourceGraph::load(&root.join("src/shaders/metal"), source)?;
        for file in graph.files.keys() {
            println!(
                "cargo:rerun-if-changed={}",
                root.join("src/shaders/metal").join(file).display()
            );
        }
        let mut interface = interfaces::get(family)?;
        if matches!(
            family,
            "fine-main"
                | "sdf-coverage"
                | "scan-count"
                | "scan-emit"
                | "filter-brush"
                | "filter-layer"
                | "filter-stack"
                | "coarse-count"
                | "coarse-particle-counts"
                | "coarse-tile-counts"
                | "coarse-tile-kinds"
                | "coarse-emit"
                | "coarse-emit-chunks"
        ) {
            // Backend-private length metadata preserves guarded raw-read semantics;
            // scene records and externally supplied binding slots remain shared.
            interface.resources.insert(
                "metal_buffer_sizes".into(),
                abi::Resource {
                    binding: 29,
                    kind: abi::Kind::Uniform,
                    size: 128,
                    count: 1,
                    internal: true,
                    fields: (0..8)
                        .map(|i| abi::Field {
                            name: char::from(b'a' + i).to_string(),
                            offset: u32::from(i) * 16,
                            lanes: 4,
                            scalar: abi::Scalar::U32,
                        })
                        .collect(),
                },
            );
            for bindings in interface.entries.values_mut() {
                bindings.push("metal_buffer_sizes".into());
            }
        }
        let recipe = serde_json::json!({"schema":1,"language":"msl","target":"macos-metallib","target_triple":env::var("TARGET").unwrap(),"family":family,"flags":metal::FLAGS,"toolchain":compiler.identity,"sources":graph.files,"abi_sha256":metal::digest(&interface.cache_bytes()),"metal_grid_buffer":30});
        let key = CacheKey::new(&[&serde_json::to_vec(&recipe)?, graph.expanded.as_bytes()]);
        let artifact = cache.get_or_compile_validated(
            &key,
            |bytes| Ok(metal::validate_container(bytes).is_ok()),
            || compiler.compile(&graph.expanded, &out.join("metal-work").join(key.hex())),
        )?;
        let output = out.join(format!("native-{family}.metallib"));
        write_changed(&output, &artifact.bytes)?;
        for entry in interface.entries.keys() {
            let bindings = abi::binding_declarations(&interface, entry)?;
            let uniforms = interface
                .resources_for(entry)?
                .into_iter()
                .filter(|(_, resource)| resource.kind == abi::Kind::Uniform)
                .map(|(_, resource)| {
                    let fields = resource
                        .fields
                        .iter()
                        .flat_map(|field| {
                            let scalar = match field.scalar {
                                abi::Scalar::U32 => 0,
                                abi::Scalar::I32 => 1,
                                abi::Scalar::F32 => 2,
                            };
                            (0..field.lanes)
                                .map(move |lane| format!("({}, {scalar})", field.offset + lane * 4))
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "UniformLayout {{ slot: {}, fields: &[{fields}] }}",
                        resource.binding
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            declarations.push_str(&format!("NativeShaderArtifact {{ format: \"metallib\", entry: {entry:?}, cache_key: {:?}, workgroup: {:?}, bindings: {bindings}, uniforms: &[{uniforms}], bytes: include_bytes!({output:?}) }},\n", key.hex(), interface.workgroup));
        }
        manifest.push(serde_json::json!({"key":key.hex(),"artifact_sha256":metal::digest(&artifact.bytes),"recipe":recipe}));
    }
    Ok(())
}
