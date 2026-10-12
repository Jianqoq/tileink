//! Common pipelines for complete application source and build-compiled filters.
//! Shader source is never generated or rewritten by this execution layer.
mod brush_invocation;
mod brushes;
#[cfg(any(feature = "dx12", feature = "vulkan"))]
pub(crate) mod dxc;
mod layout;
pub(crate) mod pipeline;
#[cfg(test)]
mod tests;
use super::Result;
#[cfg(any(feature = "dx12", feature = "vulkan"))]
use super::shader_cache as cache;
pub(crate) use brush_invocation::{Output, record as record_brush};
pub(crate) use brushes::{BrushImages, brushes_for_filter, prepare_brushes};
pub(crate) use pipeline::record as record_pipeline;
#[cfg(feature = "metal")]
pub(crate) fn config_layout() -> &'static [crate::native::shaders::UniformLayout] {
    layout::CONFIG
}
use std::rc::Rc;

#[derive(Debug)]
pub(crate) struct NativeSource {
    pub(crate) key: String,
    pub(crate) entry: Rc<str>,
    pub(crate) bindings: Vec<crate::native::shaders::Binding>,
    pub(crate) workgroup: [u32; 3],
    #[cfg(feature = "metal")]
    pub(crate) config: bool,
    #[cfg(any(feature = "dx12", feature = "vulkan"))]
    pub(crate) interface_bindings: Rc<[crate::ShaderBinding]>,
    metal: Option<Rc<str>>,
    hlsl: Option<Rc<str>>,
}
impl NativeSource {
    pub(crate) fn text(&self, metal: bool) -> Result<&str> {
        if metal {
            self.metal.as_deref()
        } else {
            self.hlsl.as_deref()
        }
        .ok_or_else(|| {
            if metal {
                "shader has no Metal source"
            } else {
                "shader has no HLSL source"
            }
            .into()
        })
    }
}
