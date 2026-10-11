//! Compile the shared runtime DXC path on every host, including Metal-only CI.
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use tileink::ShaderBindingKind;
struct NativeSource {
    entry: std::rc::Rc<str>,
    interface_bindings: std::rc::Rc<[tileink::ShaderBinding]>,
    workgroup: [u32; 3],
    hlsl: Option<String>,
}
impl NativeSource {
    fn text(&self, _metal: bool) -> Result<&str> {
        self.hlsl
            .as_deref()
            .ok_or_else(|| "shader has no HLSL source".into())
    }
}
#[allow(dead_code)]
#[path = "../src/native/runtime/program_shader/dxc.rs"]
mod runtime_dxc;
#[test]
fn missing_hlsl_source_is_reported_before_toolchain_discovery() {
    let source = NativeSource {
        entry: "paint".into(),
        interface_bindings: std::rc::Rc::from([]),
        workgroup: [256, 1, 1],
        hlsl: None,
    };
    match runtime_dxc::compile(&source, "dxil") {
        Err(error) => assert_eq!(error.to_string(), "shader has no HLSL source"),
        Ok(_) => panic!("missing source compiled"),
    }
}
#[test]
fn dynamic_user_interface_preserves_names_slots_precision_and_workgroup() {
    use tileink::{ShaderBinding as B, ShaderBindingKind as K, ShaderTextureFormat as F};
    let source = NativeSource {
        entry: "custom".into(),
        workgroup: [16, 8, 1],
        hlsl: None,
        interface_bindings: std::rc::Rc::from([
            B {
                name: "params".into(),
                slot: 7,
                kind: K::Uniform(32),
            },
            B {
                name: "atlas".into(),
                slot: 12,
                kind: K::TextureArray,
            },
            B {
                name: "images".into(),
                slot: 30,
                kind: K::TextureTable(2),
            },
            B {
                name: "config".into(),
                slot: 0,
                kind: K::Config,
            },
            B {
                name: "hdr".into(),
                slot: 2,
                kind: K::ReadWriteTexture(F::Rgba32Float),
            },
            B {
                name: "buffer".into(),
                slot: 4,
                kind: K::ReadWriteBuffer,
            },
            B {
                name: "s".into(),
                slot: 6,
                kind: K::Sampler,
            },
        ]),
    };
    let interface = runtime_dxc::interface(&source);
    assert_eq!(interface.workgroup, [16, 8, 1]);
    assert_eq!(interface.resources["params"].size, 32);
    assert!(interface.resources["params"].fields.is_empty());
    assert_eq!(interface.resources["atlas"].binding, 12);
    assert_eq!(interface.resources["images"].count, 2);
    assert_eq!(interface.resources["hdr"].size, 16);
    assert_eq!(interface.resources["buffer"].binding, 4);
    assert_eq!(interface.resources["s"].size, 0);
    assert_eq!(interface.resources["config"].fields.len(), 28);
}

#[path = "native_user_shader_compiler/process.rs"]
mod process;
