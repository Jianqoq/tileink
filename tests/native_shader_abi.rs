#[path = "../build/native/abi.rs"]
mod abi;
#[path = "../build/native/dxil_reflection.rs"]
mod dxil_reflection;

#[test]
fn incomplete_duplicate_or_incompatible_probe_inventory_is_rejected() {
    let valid: serde_json::Value =
        serde_json::from_str(include_str!("../src/shaders/probe-abi.json")).unwrap();
    abi::validate(&valid).unwrap();
    for programs in [
        serde_json::json!([]),
        serde_json::json!(["clear_words"]),
        serde_json::json!(["clear_words", "copy_words", "copy_words"]),
    ] {
        let mut wrong = valid.clone();
        wrong["programs"] = programs;
        assert!(abi::validate(&wrong).is_err());
    }
    for field in [
        "schema",
        "descriptor_set",
        "parameter_size",
        "buffer_offsets_alignment",
    ] {
        let mut wrong = valid.clone();
        wrong[field] = serde_json::json!(99);
        assert!(abi::validate(&wrong).is_err());
    }
}

#[test]
fn dxil_resource_kind_and_parameter_layout_must_match_byte_buffer_abi() {
    let abi: serde_json::Value =
        serde_json::from_str(include_str!("../src/shaders/probe-abi.json")).unwrap();
    let reflection = "; EntryFunctionName: copy_words\n; NumThreads=(64,1,1)\n; uint count; ; Offset: 0\n; uint source_offset; ; Offset: 4\n; uint destination_offset; ; Offset: 8\n; uint stride; ; Offset: 12\n; uint4 value; ; Offset: 16\n; } params; ; Offset: 0 Size: 32\n; Resource Bindings:\n; params cbuffer NA NA CB0 cb2 1\n; destination UAV byte r/w U0 u0 1\n; source texture byte r/o T0 t1 1\ntarget datalayout = \"irrelevant\"\n";
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
    wrong["descriptor_set"] = serde_json::json!(1);
    assert!(dxil_reflection::validate(reflection, "copy_words", &wrong).is_err());
}
