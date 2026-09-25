#[allow(dead_code)]
#[path = "../build/native/abi.rs"]
mod abi;
#[path = "../build/native/dxil_reflection.rs"]
mod dxil_reflection;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[allow(dead_code)]
#[path = "../build/native/interfaces.rs"]
mod interfaces;

#[test]
fn invalid_typed_interfaces_are_rejected_and_invalidate_cache_keys() {
    let valid = interfaces::get("probe").unwrap();
    abi::validate(&valid).unwrap();
    let mut wrong = valid.clone();
    wrong.entries.clear();
    assert!(abi::validate(&wrong).is_err());
    let mut wrong = valid.clone();
    wrong
        .entries
        .get_mut("copy_words")
        .unwrap()
        .push("source".into());
    assert!(abi::validate(&wrong).is_err());
    let mut wrong = valid.clone();
    wrong
        .entries
        .get_mut("copy_words")
        .unwrap()
        .push("missing".into());
    assert!(abi::validate(&wrong).is_err());
    for group in [[0, 1, 1], [1025, 1, 1], [32, 64, 1]] {
        let mut wrong = valid.clone();
        wrong.workgroup = group;
        assert!(abi::validate(&wrong).is_err());
        assert_ne!(valid.cache_bytes(), wrong.cache_bytes());
    }
    let mut changed = valid.clone();
    changed.resources.get_mut("source").unwrap().binding = 4;
    assert_ne!(valid.cache_bytes(), changed.cache_bytes());
    let mut changed = valid.clone();
    changed.resources.get_mut("params").unwrap().fields[0].name = "changed".into();
    assert_ne!(valid.cache_bytes(), changed.cache_bytes());
    assert!(interfaces::get("unknown").is_err());
}

#[test]
fn dxil_resource_kind_and_parameter_layout_must_match_byte_buffer_abi() {
    let abi = interfaces::get("probe").unwrap();
    let reflection = "; EntryFunctionName: copy_words\n; NumThreads=(64,1,1)\n; cbuffer params\n; uint count; ; Offset: 0\n; uint source_offset; ; Offset: 4\n; uint destination_offset; ; Offset: 8\n; uint stride; ; Offset: 12\n; uint4 value; ; Offset: 16\n; } params; ; Offset: 0\n; } params; ; Offset: 0 Size: 32\n; Resource Bindings:\n; params cbuffer NA NA CB0 cb2 1\n; destination UAV byte r/w U0 u0 1\n; source texture byte r/o T0 t1 1\ntarget datalayout = \"irrelevant\"\n";
    dxil_reflection::validate(reflection, "copy_words", &abi).unwrap();
    for (from, to) in [
        ("UAV byte r/w", "UAV float 2d"),
        ("texture byte r/o", "texture struct r/o"),
        ("Offset: 16", "Offset: 20"),
        ("cb2", "cb2,space1"),
    ] {
        assert!(
            dxil_reflection::validate(&reflection.replace(from, to), "copy_words", &abi).is_err()
        );
    }
    // Extra resource arrays must not disappear from the reflected binding count.
    for count in ["2", "unbounded"] {
        let extra = reflection.replace(
            "target datalayout",
            &format!("; extra texture byte r/o T1 t3 {count}\ntarget datalayout"),
        );
        assert!(dxil_reflection::validate(&extra, "copy_words", &abi).is_err());
    }
    let mut wrong = abi.clone();
    wrong.descriptor_set = 1;
    assert!(dxil_reflection::validate(reflection, "copy_words", &wrong).is_err());
}

#[test]
fn range_scatter_has_two_buffers_no_uniforms_and_256_threads() {
    let valid = interfaces::get("range-scatter").unwrap();
    abi::validate(&valid).unwrap();
    let reflection = "; EntryFunctionName: range_scatter\n; NumThreads=(256,1,1)\n; Resource Bindings:\n; destination UAV byte r/w U0 u0 1\n; source texture byte r/o T0 t1 1\ntarget datalayout = irrelevant\n";
    dxil_reflection::validate(reflection, "range_scatter", &valid).unwrap();
    for (from, to) in [
        ("256,1,1", "64,1,1"),
        ("t1 1", "t0 1"),
        ("texture byte r/o", "UAV byte r/w"),
    ] {
        assert!(
            dxil_reflection::validate(&reflection.replace(from, to), "range_scatter", &valid)
                .is_err()
        );
    }
    let extra = reflection.replace(
        "target datalayout",
        "; params cbuffer NA NA CB0 cb2 1\ntarget datalayout",
    );
    assert!(dxil_reflection::validate(&extra, "range_scatter", &valid).is_err());
}

#[test]
fn compute_layout_rejects_duplicate_slots_and_invalid_uniform_fields() {
    let valid = interfaces::get("cumsum").unwrap();
    abi::validate(&valid).unwrap();
    assert!(
        abi::binding_declarations(&valid, "cumsum_prefix_chunks")
            .unwrap()
            .contains("slot: 31")
    );
    let mut wrong = valid.clone();
    wrong.resources.get_mut("backdrops").unwrap().binding = 1;
    assert!(abi::validate(&wrong).is_err());
    let mut wrong = valid.clone();
    wrong.resources.get_mut("config").unwrap().fields[1].offset = 8;
    assert!(abi::validate(&wrong).is_err());
    let mut wrong = valid.clone();
    wrong.resources.get_mut("dispatch_grid").unwrap().internal = false;
    assert!(abi::validate(&wrong).is_err());
}

#[test]
fn shader_interfaces_do_not_depend_on_abi_json_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-abi.json"),
            "{}",
            path.display()
        );
    }
}

#[test]
fn uniform_scalar_types_are_reflected_and_invalidate_shader_cache_keys() {
    // Signed offsets and floating filter parameters must never alias uint metadata.
    let base = interfaces::get("probe").unwrap();
    let mut typed = base.clone();
    typed.resources.get_mut("params").unwrap().fields[0].scalar = abi::Scalar::I32;
    typed.resources.get_mut("params").unwrap().fields[4].scalar = abi::Scalar::F32;
    abi::validate(&typed).unwrap();
    assert_ne!(base.cache_bytes(), typed.cache_bytes());
    let reflection = "; EntryFunctionName: clear_words\n; NumThreads=(64,1,1)\n; cbuffer params\n; int count; ; Offset: 0\n; uint source_offset; ; Offset: 4\n; uint destination_offset; ; Offset: 8\n; uint stride; ; Offset: 12\n; float4 value; ; Offset: 16\n; } params; ; Offset: 0 Size: 32\n; Resource Bindings:\n; params cbuffer NA NA CB0 cb2 1\n; destination UAV byte r/w U0 u0 1\ntarget datalayout = irrelevant\n";
    dxil_reflection::validate(reflection, "clear_words", &typed).unwrap();
    for (from, to) in [("int count", "uint count"), ("float4 value", "uint4 value")] {
        assert!(
            dxil_reflection::validate(&reflection.replace(from, to), "clear_words", &typed)
                .is_err()
        );
    }
    for scalar in [abi::Scalar::U32, abi::Scalar::I32] {
        let mut changed = typed.clone();
        changed.resources.get_mut("params").unwrap().fields[4].scalar = scalar;
        assert_ne!(typed.cache_bytes(), changed.cache_bytes());
    }
}

#[test]
fn texture_table_count_and_register_ranges_are_explicit() {
    let mut description = interfaces::get("texture-table-validation").unwrap();
    abi::validate(&description).unwrap();
    let count = description.resources["texture_table"].count;
    let original = description.cache_bytes();
    description
        .resources
        .get_mut("texture_table")
        .unwrap()
        .count = 1;
    assert_ne!(description.cache_bytes(), original);
    abi::validate(&description).unwrap();
    description
        .resources
        .get_mut("texture_table")
        .unwrap()
        .count = 0;
    assert!(abi::validate(&description).is_err());
    description
        .resources
        .get_mut("texture_table")
        .unwrap()
        .count = count;
    description
        .resources
        .get_mut("texture_table")
        .unwrap()
        .binding = 0;
    assert!(abi::validate(&description).is_err());
    description
        .resources
        .get_mut("texture_table")
        .unwrap()
        .binding = 29;
    description.resources.get_mut("requests").unwrap().binding = 30;
    assert!(
        abi::validate(&description).is_err(),
        "SRV ranges overlap even when starting bindings differ"
    );
}

#[test]
fn metal_internal_lengths_are_typed_bounded_and_part_of_the_cache_contract() {
    let mut interface = interfaces::get("probe").unwrap();
    let original = interface.cache_bytes();
    let sizes = abi::Resource {
        binding: 29,
        kind: abi::Kind::Uniform,
        size: 128,
        count: 1,
        internal: true,
        fields: (0..8u8)
            .map(|i| abi::Field {
                name: char::from(b'a' + i).to_string(),
                offset: u32::from(i) * 16,
                lanes: 4,
                scalar: abi::Scalar::U32,
            })
            .collect(),
    };
    interface
        .resources
        .insert("metal_buffer_sizes".into(), sizes);
    interface
        .entries
        .get_mut("copy_words")
        .unwrap()
        .push("metal_buffer_sizes".into());
    abi::validate(&interface).unwrap();
    assert_ne!(interface.cache_bytes(), original);
    for mode in 0..4 {
        let mut wrong = interface.clone();
        let resource = wrong.resources.get_mut("metal_buffer_sizes").unwrap();
        match mode {
            0 => resource.binding = 28,
            1 => resource.size = 124,
            2 => resource.fields[0].scalar = abi::Scalar::F32,
            3 => resource.fields[1].offset = 0,
            _ => unreachable!(),
        }
        assert!(abi::validate(&wrong).is_err(), "malformed metadata {mode}");
    }
}
