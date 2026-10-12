#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[allow(
    dead_code,
    reason = "Shared compiler helpers expose different entry points in standalone tests and backend builds."
)]
#[path = "../build/native/abi.rs"]
mod abi;
#[allow(
    dead_code,
    reason = "Shared compiler helpers expose different entry points in standalone tests and backend builds."
)]
#[path = "../build/native/dxil_reflection.rs"]
mod dxil;
#[test]
fn user_shader_interfaces_support_float_storage_and_buffer_only_passes() {
    use abi::{Interface, Kind, Resource};
    use std::collections::BTreeMap;
    for (kind, size, row) in [
        (Kind::TextureWrite, 8, "UAV f32 2d U0 u2 1"),
        (Kind::TextureWrite, 16, "UAV f32 2d U0 u2 1"),
        (Kind::Write, 4, "UAV byte r/w U0 u2 1"),
    ] {
        let interface = Interface {
            workgroup: [8, 8, 1],
            descriptor_set: 0,
            resources: BTreeMap::from([(
                "result".into(),
                Resource {
                    binding: 2,
                    kind,
                    size,
                    count: 1,
                    fields: vec![],
                    internal: false,
                },
            )]),
            entries: BTreeMap::from([("custom".into(), vec!["result".into()])]),
        };
        let text = format!(
            "; EntryFunctionName: custom\n; NumThreads=(8,8,1)\n; Resource Bindings:\n; Name Type Format Dim ID HLSL Bind Count\n; result {row}\n\ntarget datalayout"
        );
        dxil::validate_user(&text, "custom", &interface).unwrap();
        assert!(
            dxil::validate_user(&text.replace("result ", "unknown "), "custom", &interface)
                .is_err()
        );
    }
}
#[test]
fn user_shader_reflection_allows_inactive_resources_but_rejects_wrong_launch_or_bindings() {
    use abi::{Interface, Kind, Resource};
    use std::collections::BTreeMap;
    let resource = |binding, kind| Resource {
        binding,
        kind,
        size: 4,
        count: 1,
        fields: vec![],
        internal: false,
    };
    let interface = Interface {
        workgroup: [256, 1, 1],
        descriptor_set: 0,
        resources: BTreeMap::from([
            ("output_image".into(), resource(3, Kind::TextureWrite)),
            ("uniforms".into(), resource(10, Kind::Read)),
        ]),
        entries: BTreeMap::from([(
            "paint".into(),
            vec!["output_image".into(), "uniforms".into()],
        )]),
    };
    let text = "; EntryFunctionName: paint\n; NumThreads=(256,1,1)\n; Resource Bindings:\n; Name Type Format Dim ID HLSL Bind Count\n; output_image UAV f32 2d U0 u3 1\n\ntarget datalayout";
    dxil::validate_user(text, "paint", &interface).unwrap();
    assert!(dxil::validate(text, "paint", &interface).is_err());
    for text in [
        text.replace("256,1,1", "128,1,1"),
        text.replace("u3", "u4"),
        text.replace("f32 2d", "f32 2darray"),
        text.replace("UAV", "texture"),
        text.replace("output_image", "unexpected"),
        text.replace(" 1\n", " 2\n"),
    ] {
        assert!(
            dxil::validate_user(&text, "paint", &interface).is_err(),
            "{text}"
        );
    }
}

#[test]
fn raw_user_constant_blocks_validate_byte_size_without_builtin_field_names() {
    use abi::{Interface, Kind, Resource};
    let interface = Interface {
        workgroup: [8, 8, 1],
        descriptor_set: 0,
        resources: std::collections::BTreeMap::from([
            (
                "params".into(),
                Resource {
                    binding: 0,
                    kind: Kind::Uniform,
                    size: 32,
                    count: 1,
                    fields: vec![],
                    internal: false,
                },
            ),
            (
                "output".into(),
                Resource {
                    binding: 3,
                    kind: Kind::TextureWrite,
                    size: 4,
                    count: 1,
                    fields: vec![],
                    internal: false,
                },
            ),
        ]),
        entries: std::collections::BTreeMap::from([(
            "custom".into(),
            vec!["params".into(), "output".into()],
        )]),
    };
    let text = "; EntryFunctionName: custom\n; NumThreads=(8,8,1)\n; cbuffer params\n; float4 tint; Offset: 0\n; uint4 indices; Offset: 16\n; } params; Offset: 0 Size: 32\n; Resource Bindings:\n; Name Type Format Dim ID HLSL Bind Count\n; params cbuffer NA NA CB0 cb0 1\n; output UAV f32 2d U0 u3 1\n\ntarget datalayout";
    dxil::validate_user(text, "custom", &interface).unwrap();
    assert!(abi::validate(&interface).is_err());
    assert!(
        dxil::validate_user(&text.replace("Size: 32", "Size: 16"), "custom", &interface).is_err()
    );
}

#[allow(
    dead_code,
    reason = "Shared compiler helpers expose different entry points in standalone tests and backend builds."
)]
#[path = "../build/native/spirv.rs"]
mod spirv;

#[path = "native_user_shader_reflection/spirv_access.rs"]
mod spirv_access;
