//! Build-time native shader products. Loading these bytes does not invoke DXC.
//! Driver-specific pipeline creation/caching belongs to the native adapters.

#[derive(Clone, Copy, Debug)]
pub struct NativeShaderArtifact {
    pub format: &'static str,
    pub entry: &'static str,
    pub cache_key: &'static str,
    pub workgroup: [u32; 3],
    pub(crate) bindings: &'static [Binding],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/tileink_native_artifacts.rs"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindingKind {
    Uniform,
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub slot: u32,
    pub kind: BindingKind,
    pub size: u32,
    pub internal: bool,
}
