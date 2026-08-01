use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

use naga::{
    ResourceBinding, ShaderStage,
    back::hlsl::{
        BindTarget, Options, PipelineOptions, SamplerHeapBindTargets, SamplerIndexBufferKey,
        ShaderModel, Writer,
    },
    valid::{Capabilities, ValidationFlags, Validator},
};

use crate::{
    dxc,
    dxil_manifest::{
        Dx12ResourceClass, FINE_DXIL_BINDINGS, FINE_DXIL_ENTRY_POINTS, FINE_DXIL_WORKGROUP_SIZE,
    },
    shader_variants::patch_image_resource_shader_source,
};

pub(crate) fn generate(fine_portable_source: &str, out_dir: &Path) {
    println!("cargo:rerun-if-env-changed=TILEINK_DXIL_PRECOMPILE");
    dxc::emit_discovery_inputs();

    let generated = out_dir.join("tileink_dxil.rs");
    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some("windows".as_ref())
        || env::var("TILEINK_DXIL_PRECOMPILE").ok().as_deref() == Some("0")
    {
        write_generated(&generated, &[]);
        return;
    }

    let Some(dxc) = dxc::find() else {
        println!(
            "cargo:warning=DXC was not found; Tileink will use runtime WGSL compilation on DX12"
        );
        write_generated(&generated, &[]);
        return;
    };
    dxc::emit_toolchain_inputs(&dxc);

    match compile_fine_variants(fine_portable_source, out_dir, &dxc) {
        Ok(outputs) => write_generated(&generated, &outputs),
        Err(error) => panic!("failed to precompile Tileink DXIL: {error}"),
    }
}

fn compile_fine_variants(
    fine_portable_source: &str,
    out_dir: &Path,
    dxc: &Path,
) -> Result<Vec<(String, PathBuf)>, String> {
    let source = patch_image_resource_shader_source(fine_portable_source, true);
    let module =
        naga::front::wgsl::parse_str(&source).map_err(|error| error.emit_to_string(&source))?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|error| format!("WGSL validation failed: {error:?}"))?;
    let hlsl_options = fine_hlsl_options();

    let mut jobs = Vec::with_capacity(FINE_DXIL_ENTRY_POINTS.len());
    for entry_point in FINE_DXIL_ENTRY_POINTS {
        let workgroup_size = module
            .entry_points
            .iter()
            .find(|candidate| {
                candidate.name == entry_point && candidate.stage == ShaderStage::Compute
            })
            .map(|candidate| tuple(candidate.workgroup_size))
            .ok_or_else(|| format!("WGSL compute entry point {entry_point} is missing"))?;
        if workgroup_size != FINE_DXIL_WORKGROUP_SIZE {
            return Err(format!(
                "WGSL entry point {entry_point} has workgroup size {workgroup_size:?}, expected {FINE_DXIL_WORKGROUP_SIZE:?}"
            ));
        }
        let pipeline_options = PipelineOptions {
            entry_point: Some((ShaderStage::Compute, entry_point.to_string())),
        };
        let mut hlsl = String::new();
        let mut writer = Writer::new(&mut hlsl, &hlsl_options, &pipeline_options);
        let mut reflection = writer
            .write(&module, &info, None)
            .map_err(|error| format!("HLSL translation for {entry_point} failed: {error:?}"))?;
        let reflected_entry_point = reflection
            .entry_point_names
            .pop()
            .ok_or_else(|| format!("HLSL translation omitted {entry_point}"))?
            .map_err(|error| format!("HLSL reflection for {entry_point} failed: {error}"))?;
        let stem = entry_point.replace('_', "-");
        let hlsl_path = out_dir.join(format!("tileink-{stem}-sm60.hlsl"));
        let dxil_path = out_dir.join(format!("tileink-{stem}-sm60.dxil"));
        fs::write(&hlsl_path, hlsl)
            .map_err(|error| format!("could not write {}: {error}", hlsl_path.display()))?;
        jobs.push((
            entry_point.to_string(),
            reflected_entry_point,
            hlsl_path,
            dxil_path,
        ));
    }

    // DXC compilation dominates a clean build. Independent entry points are compiled concurrently;
    // Cargo then reuses these OUT_DIR artifacts until shader sources or the compiler change.
    let results = thread::scope(|scope| {
        jobs.into_iter()
            .map(
                |(entry_point, reflected_entry_point, hlsl_path, dxil_path)| {
                    let dxc = dxc.to_path_buf();
                    scope.spawn(move || {
                        compile_with_dxc(
                            &dxc,
                            &entry_point,
                            &reflected_entry_point,
                            &hlsl_path,
                            &dxil_path,
                        )?;
                        Ok((entry_point, dxil_path))
                    })
                },
            )
            .collect::<Vec<_>>()
            .into_iter()
            .map(|job| job.join().map_err(|_| "DXC worker panicked".to_string())?)
            .collect::<Result<Vec<_>, String>>()
    })?;
    Ok(results)
}

fn tuple(value: [u32; 3]) -> (u32, u32, u32) {
    (value[0], value[1], value[2])
}

fn compile_with_dxc(
    dxc: &Path,
    entry_point: &str,
    reflected_entry_point: &str,
    hlsl_path: &Path,
    dxil_path: &Path,
) -> Result<(), String> {
    let output = Command::new(dxc)
        .args([
            "-E",
            reflected_entry_point,
            "-T",
            "cs_6_0",
            "-HV",
            "2018",
            "-no-warnings",
            "-Ges",
            "-O3",
            "-Fo",
        ])
        .arg(dxil_path)
        .arg(hlsl_path)
        .output()
        .map_err(|error| format!("could not launch {}: {error}", dxc.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "DXC failed for {entry_point}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn fine_hlsl_options() -> Options {
    let mut options = Options {
        shader_model: ShaderModel::V6_0,
        fake_missing_bindings: false,
        sampler_heap_target: SamplerHeapBindTargets {
            standard_samplers: bind_target(0, None),
            comparison_samplers: bind_target(2048, None),
        },
        ..Options::default()
    };
    let groups = FINE_DXIL_BINDINGS
        .iter()
        .map(|binding| binding.group)
        .collect::<BTreeSet<_>>();
    let (mut cbv, mut srv, mut uav) = (0, 0, 0);
    for group in groups {
        for binding in FINE_DXIL_BINDINGS
            .iter()
            .filter(|binding| binding.group == group && binding.class != Dx12ResourceClass::Sampler)
        {
            let register = match binding.class {
                Dx12ResourceClass::ConstantBuffer => &mut cbv,
                Dx12ResourceClass::ShaderResource => &mut srv,
                Dx12ResourceClass::UnorderedAccess => &mut uav,
                Dx12ResourceClass::Sampler => unreachable!(),
            };
            options.binding_map.insert(
                ResourceBinding {
                    group,
                    binding: binding.binding,
                },
                bind_target(*register, (binding.count > 1).then_some(binding.count)),
            );
            *register += binding.count;
        }

        let samplers = FINE_DXIL_BINDINGS
            .iter()
            .filter(|binding| binding.group == group && binding.class == Dx12ResourceClass::Sampler)
            .collect::<Vec<_>>();
        for (index, binding) in samplers.iter().enumerate() {
            options.binding_map.insert(
                ResourceBinding {
                    group,
                    binding: binding.binding,
                },
                BindTarget {
                    space: 255,
                    register: index as u32,
                    ..BindTarget::default()
                },
            );
        }
        if !samplers.is_empty() {
            options
                .sampler_buffer_binding_map
                .insert(SamplerIndexBufferKey { group }, bind_target(srv, None));
            srv += 1;
        }
    }
    options
}

fn bind_target(register: u32, binding_array_size: Option<u32>) -> BindTarget {
    BindTarget {
        space: 0,
        register,
        binding_array_size,
        dynamic_storage_buffer_offsets_index: None,
        restrict_indexing: false,
    }
}

fn write_generated(path: &Path, outputs: &[(String, PathBuf)]) {
    let mut source = String::from("// @generated by build/dxil.rs\n");
    source.push_str("pub(crate) static PRECOMPILED_FINE_DXIL: &[PrecompiledDxil] = &[\n");
    for (entry_point, _) in outputs {
        let stem = entry_point.replace('_', "-");
        source.push_str(&format!(
            "    PrecompiledDxil {{ entry_point: {entry_point:?}, workgroup_size: crate::wgpu::dxil_manifest::FINE_DXIL_WORKGROUP_SIZE, bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/tileink-{stem}-sm60.dxil\")) }},\n"
        ));
    }
    source.push_str("];\n");
    fs::write(path, source).unwrap();
}
