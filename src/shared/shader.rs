//! User-owned native compute shaders. Sources are compiled verbatim: Tileink
//! supplies resources and scheduling, never an expression language or interpreter.
mod config;
mod encode;
mod pipeline;
#[cfg(tileink_native_runtime)]
pub(crate) use pipeline::ShaderKernelCode;
pub use pipeline::{
    ShaderBinding, ShaderBindingKind, ShaderConfigDescriptor, ShaderDispatch, ShaderExtent,
    ShaderKernel, ShaderPass, ShaderPipeline, ShaderResource, ShaderResourceKind, ShaderSampler,
    ShaderTextureFormat,
};
#[cfg(test)]
mod tests;
pub use config::ShaderConfig;
use std::{fmt, rc::Rc};

/// Complete source files for the backends the application supports.
/// Includes must be resolved by the application before submission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderSources {
    pub metal: Option<String>,
    pub hlsl: Option<String>,
}

/// Declares the immutable input dependency used for allocation and retained damage.
/// The shader must respect this declaration; raw native code is not rewritten.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShaderInput {
    None,
    /// Read only the pixel being written.
    Current,
    /// Maximum per-axis displacement in logical canvas units. Dependency tracking
    /// additionally includes the bilinear sample footprint.
    Sampled {
        radius: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderError {
    MissingSource,
    InvalidEntry,
    InvalidUniformSize,
    UniformCount,
    BrushReadsInput,
    InvalidSampleRadius,
    InvalidOutputOutset,
    InvalidResource,
    InvalidBinding,
    InvalidDispatch,
}
impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingSource => "provide at least one nonempty native shader source",
            Self::InvalidEntry => "shader entry must be an ASCII identifier",
            Self::InvalidUniformSize => "shader uniform size exceeds native byte addressing",
            Self::UniformCount => "shader uniform word count does not match its declaration",
            Self::BrushReadsInput => "procedural brushes cannot read filter input",
            Self::InvalidSampleRadius => "shader sampling radius must be finite and nonnegative",
            Self::InvalidOutputOutset => "shader output outset must be finite and nonnegative",
            Self::InvalidResource => "invalid shader pipeline resource or uninitialized output",
            Self::InvalidBinding => {
                "shader pipeline binding type, slot, access, or initialization mismatch"
            }
            Self::InvalidDispatch => "invalid shader workgroup, dispatch, or extent",
        })
    }
}
impl std::error::Error for ShaderError {}

/// A complete native compute kernel, with a fixed 256x1x1 workgroup and the
/// resource ABI documented by `ShaderConfig`. Output is premultiplied sRGB RGBA8.
/// Compilation errors are reported by rendering; there is no fallback shader.
#[derive(Clone, Debug)]
pub struct ShaderProgram {
    pub(crate) sources: Rc<ShaderSources>,
    pub(crate) entry: Rc<str>,
    pub(crate) input: ShaderInput,
    pub(crate) uniform_words: usize,
    pub(crate) kernel: ShaderKernel,
}
impl ShaderProgram {
    pub fn new(
        sources: ShaderSources,
        entry: &str,
        input: ShaderInput,
        uniform_words: usize,
    ) -> Result<Self, ShaderError> {
        // The public kernel constructor owns source, entry and binding validation.
        let sources = Rc::new(sources);
        let kernel = pipeline::program_kernel(sources.clone(), entry, input)?;
        if uniform_words > u32::MAX as usize / 4 {
            return Err(ShaderError::InvalidUniformSize);
        }
        if let ShaderInput::Sampled { radius } = input
            && (!radius.is_finite() || radius < 0.0)
        {
            return Err(ShaderError::InvalidSampleRadius);
        }
        Ok(Self {
            kernel,
            sources,
            entry: entry.into(),
            input,
            uniform_words,
        })
    }
    pub fn uniform_words(&self) -> usize {
        self.uniform_words
    }
    pub fn samples_input(&self) -> bool {
        matches!(self.input, ShaderInput::Sampled { .. })
    }
    pub fn sample_radius(&self) -> f32 {
        if let ShaderInput::Sampled { radius } = self.input {
            radius
        } else {
            0.0
        }
    }
    pub(crate) fn reads_input(&self) -> bool {
        self.input != ShaderInput::None
    }
    pub fn brush(&self, uniforms: impl AsRef<[u32]>) -> Result<ProgramBrush, ShaderError> {
        if self.reads_input() {
            return Err(ShaderError::BrushReadsInput);
        }
        let uniforms = uniforms.as_ref();
        self.validate_uniforms(uniforms)?;
        Ok(ProgramBrush(BrushInstance {
            program: self.clone(),
            uniforms: uniforms.into(),
            transform: super::brush::IDENTITY_TRANSFORM,
        }))
    }
    /// Convenience constructor for a standard single-pass ShaderPipeline.
    /// There is no separate Program filter or execution path.
    pub fn filter(&self, uniforms: impl AsRef<[u32]>) -> Result<ShaderPipeline, ShaderError> {
        let uniforms = uniforms.as_ref();
        self.validate_uniforms(uniforms)?;
        pipeline::program_pipeline(self, uniforms)
    }
    fn validate_uniforms(&self, uniforms: &[u32]) -> Result<(), ShaderError> {
        if uniforms.len() != self.uniform_words {
            return Err(ShaderError::UniformCount);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(crate) struct BrushInstance {
    pub(crate) program: ShaderProgram,
    pub(crate) uniforms: Rc<[u32]>,
    pub(crate) transform: [f32; 6],
}
impl BrushInstance {
    fn with_uniforms(&self, uniforms: &[u32]) -> Result<Self, ShaderError> {
        self.program.validate_uniforms(uniforms)?;
        Ok(Self {
            uniforms: uniforms.into(),
            ..self.clone()
        })
    }
}
/// A native compute paint, rasterized on the GPU over its draw bounds before compositing.
#[derive(Clone, Debug)]
pub struct ProgramBrush(pub(crate) BrushInstance);
impl ProgramBrush {
    pub fn with_uniforms(&self, uniforms: impl AsRef<[u32]>) -> Result<Self, ShaderError> {
        Ok(Self(self.0.with_uniforms(uniforms.as_ref())?))
    }
}
impl From<ProgramBrush> for super::brush::Brush {
    fn from(brush: ProgramBrush) -> Self {
        Self::Program(brush)
    }
}
fn sampling_outset(transform: [f32; 6], radius: f32) -> u32 {
    (transformed_radius(transform, radius) + 1.0).ceil() as u32
}
fn transformed_radius(transform: [f32; 6], radius: f32) -> f64 {
    if radius == 0.0 {
        return 0.0;
    }
    let [a, b, c, d, _, _] = transform.map(f64::from);
    let det = a * d - b * c;
    let scale = ((d.abs() + c.abs()).max(b.abs() + a.abs())) / det.abs();
    scale * f64::from(radius)
}
