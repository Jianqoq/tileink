//! Explicit native workloads. This is a resource/dispatch description, not a
//! shader language: kernel source, loops and algorithms belong to the caller.
#[cfg(all(test, tileink_native_runtime))]
mod compiled_tests;
#[cfg(test)]
mod tests;
mod validate;
use super::{ShaderError, ShaderInput, ShaderSources};
use std::rc::Rc;

/// Storage precision; intermediate floats are neither clamped nor quantized to RGBA8.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShaderTextureFormat {
    #[default]
    Rgba8Unorm,
    Rgba16Float,
    Rgba32Float,
}
impl ShaderTextureFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8Unorm => 4,
            Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
        }
    }
}

/// Texture/dispatch extent relative to the physical filter surface. Division
/// rounds up, so odd dimensions retain their last row/column at every level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderExtent {
    Surface { divisor: [u32; 2] },
    Fixed([u32; 2]),
}
impl ShaderExtent {
    pub const FULL: Self = Self::Surface { divisor: [1, 1] };
    pub const fn downsample(factor: u32) -> Self {
        Self::Surface {
            divisor: [factor; 2],
        }
    }
    pub(crate) fn resolve(self, surface: [u32; 2]) -> [u32; 2] {
        match self {
            Self::Surface { divisor } => std::array::from_fn(|i| surface[i].div_ceil(divisor[i])),
            Self::Fixed(size) => size,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderSampler {
    Nearest,
    Linear,
}

/// Owned data is an immutable snapshot; External resources are invocation views.
/// Replace a resource with `with_resource`
/// to change parameters/images and invalidate retained output normally.
#[derive(Clone, Debug)]
pub enum ShaderResource {
    /// Immutable invocation input. Standalone filters bind the layer/backdrop at
    /// index 0; graph primitives bind their ordered `inputs` without CPU copies.
    /// Indices must be unique and contiguous from zero across resource descriptors.
    Input(usize),
    /// Final premultiplied-sRGB RGBA8 image, separate from Input.
    Output,
    /// A 112-byte ShaderConfig for this extent. Pipeline dispatches use ordinary
    /// 2D thread coordinates except for explicit Pixels dispatch. `inverse_*` maps from
    /// full-resolution surface coordinates to logical canvas coordinates.
    Config(ShaderConfigDescriptor),
    /// An arbitrary native constant block, bound with `ShaderBindingKind::Uniform`.
    Uniform(Rc<[u8]>),
    /// A typed invocation resource supplied to `NativeShaderBatch::record`.
    /// Indices are contiguous from zero; handles are resolved anew for every batch.
    External {
        index: usize,
        kind: ShaderResourceKind,
    },
    /// Read-only, tightly packed texels, including optional float/HDR data.
    Image {
        size: [u32; 2],
        format: ShaderTextureFormat,
        bytes: Rc<[u8]>,
    },
    /// GPU intermediate. Its first writer must initialize every texel that any
    /// subsequent pass reads. Storage is pooled and has unspecified old contents.
    Texture {
        extent: ShaderExtent,
        format: ShaderTextureFormat,
    },
    /// Raw words, uploaded before execution; may be bound read-only or writable.
    /// Writes are transient to this invocation and never mutate the CPU snapshot.
    Buffer(Rc<[u32]>),
    Sampler(ShaderSampler),
    /// Validated compact tile IDs for Pixels dispatch; a dummy word otherwise.
    ActiveTiles,
}

mod program;
pub(super) use program::{kernel as program_kernel, pipeline as program_pipeline};
mod resource;
pub use resource::{ShaderConfigDescriptor, ShaderResourceKind};

#[cfg(tileink_native_runtime)]
mod compiled;
mod kernel;
#[cfg(tileink_native_runtime)]
pub(crate) use kernel::ShaderKernelCode;
pub use kernel::{ShaderBinding, ShaderBindingKind, ShaderKernel};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderDispatch {
    /// Linear 256-lane pixel work over the current output region, optionally
    /// compacted into 16x16 tiles. Uses ShaderConfig and ActiveTiles resources;
    /// kernels must guard the logical region and subtract the output origin.
    Pixels,
    /// Cover an extent with ceil(extent/workgroup) groups. Kernels must guard
    /// rounded-up edge threads. Z workgroup size must be one for this mode.
    Image(ShaderExtent),
    /// Cover the current filter output region, mapped into this extent. Outset
    /// expands it in full-resolution physical pixels before scaling. Config's
    /// region origin must be added to dispatch-thread coordinates. Give earlier
    /// passes sufficient halo for every later read; textures retain stable phase.
    Region { extent: ShaderExtent, outset: u32 },
    /// Explicit GPU group counts for reductions, shared memory, and buffer work.
    Groups([u32; 3]),
}

#[derive(Clone, Debug)]
pub struct ShaderPass {
    pub kernel: ShaderKernel,
    /// (kernel slot, pipeline resource index). No writable aliases in one pass.
    pub bindings: Vec<(u32, usize)>,
    pub dispatch: ShaderDispatch,
}

/// Ordered native GPU passes with automatic resource leases and barriers. Input
/// dependencies cover the whole filter domain so reductions and multiresolution
/// sampling remain correct under retained updates. No CPU readback between passes.
#[derive(Clone)]
pub struct ShaderPipeline {
    pub(crate) resources: Rc<[ShaderResource]>,
    pub(crate) passes: Rc<[ShaderPass]>,
    pub(crate) transform: [f32; 6],
    pub(crate) input_dependency: Option<ShaderInput>,
    pub(crate) output_outset: f32,
    identity: u64,
}
fn next_identity() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |n| n.checked_add(1),
    )
    .expect("shader pipeline identity exhausted")
}
impl std::fmt::Debug for ShaderPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Immutable snapshots need only a stable identity in execution plan
        // fingerprints. Printing texture bytes and source on every frame would
        // turn a cached shader into a CPU read/format of its complete input data.
        f.debug_struct("ShaderPipeline")
            .field("identity", &self.identity)
            .field("resources", &self.resources.len())
            .field("passes", &self.passes.len())
            .field("transform", &self.transform)
            .field("input_dependency", &self.input_dependency)
            .field("output_outset", &self.output_outset)
            .finish()
    }
}
impl ShaderPipeline {
    /// Immutable declarations, available for inspecting and rebuilding a plan.
    pub fn resources(&self) -> &[ShaderResource] {
        &self.resources
    }
    pub fn passes(&self) -> &[ShaderPass] {
        &self.passes
    }

    pub fn new(
        resources: Vec<ShaderResource>,
        passes: Vec<ShaderPass>,
    ) -> Result<Self, ShaderError> {
        validate::pipeline(&resources, &passes)?;
        Ok(Self {
            resources: resources.into(),
            passes: passes.into(),
            transform: super::super::brush::IDENTITY_TRANSFORM,
            input_dependency: None,
            output_outset: 0.0,
            identity: next_identity(),
        })
    }
    pub fn with_resource(
        &self,
        index: usize,
        resource: ShaderResource,
    ) -> Result<Self, ShaderError> {
        let mut resources = self.resources.to_vec();
        *resources
            .get_mut(index)
            .ok_or(ShaderError::InvalidResource)? = resource;
        validate::pipeline(&resources, &self.passes)?;
        Ok(Self {
            resources: resources.into(),
            identity: next_identity(),
            ..self.clone()
        })
    }
    /// Native invocations bind external resources by index, independently of slots.
    pub fn external_count(&self) -> usize {
        self.resources
            .iter()
            .filter(|r| matches!(r, ShaderResource::External { .. }))
            .count()
    }
    pub(crate) fn is_canvas_filter(&self) -> bool {
        self.external_count() == 0
            && self
                .resources
                .iter()
                .any(|r| matches!(r, ShaderResource::Output))
    }
    /// Declare point or bounded sampled input dependencies. This never rewrites
    /// source; whole-domain dependencies remain the default for generic pipelines.
    pub fn with_input_dependency(&self, input: ShaderInput) -> Result<Self, ShaderError> {
        if let ShaderInput::Sampled { radius } = input
            && (!radius.is_finite() || radius < 0.0)
        {
            return Err(ShaderError::InvalidSampleRadius);
        }
        Ok(Self {
            input_dependency: Some(input),
            ..self.clone()
        })
    }
    pub(crate) fn reads_input(&self) -> bool {
        self.input_count() != 0
    }
    /// Number of immutable GPU inputs required by each invocation.
    pub fn input_count(&self) -> usize {
        self.resources
            .iter()
            .filter(|r| matches!(r, ShaderResource::Input(_)))
            .count()
    }
    /// Declare an aggregate maximum input displacement for the entire pipeline,
    /// in logical canvas units. All passes, interpolation and buffer-derived
    /// accesses must respect it. Without this declaration, dependencies are global.
    pub fn with_input_radius(&self, radius: f32) -> Result<Self, ShaderError> {
        if !radius.is_finite() || radius < 0.0 {
            return Err(ShaderError::InvalidSampleRadius);
        }
        Ok(Self {
            input_dependency: Some(ShaderInput::Sampled { radius }),
            ..self.clone()
        })
    }
    pub(crate) fn sample_outset(&self) -> Option<i32> {
        self.input_dependency.map(|input| match input {
            ShaderInput::Sampled { radius } => super::sampling_outset(self.transform, radius),
            _ => 0,
        })
    }
    /// Maximum output growth on each side of the source geometry, in logical
    /// canvas units. Independent from read radius: shadows/offsets need both.
    /// Graph primitive regions remain explicit clips, as for built-in filters.
    pub fn with_output_outset(&self, outset: f32) -> Result<Self, ShaderError> {
        if !outset.is_finite() || outset < 0.0 {
            return Err(ShaderError::InvalidOutputOutset);
        }
        Ok(Self {
            output_outset: outset,
            ..self.clone()
        })
    }
    pub(crate) fn output_outset(&self) -> i32 {
        super::transformed_radius(self.transform, self.output_outset).ceil() as i32
    }
    pub(crate) fn requires_output_scratch(&self) -> bool {
        let Some(output) = self
            .resources
            .iter()
            .position(|r| matches!(r, ShaderResource::Output))
        else {
            return false;
        };
        let first_output = self
            .passes
            .iter()
            .position(|p| p.bindings.iter().any(|&(_, r)| r == output))
            .unwrap();
        self.passes[first_output..].iter().any(|p| {
            p.bindings
                .iter()
                .any(|&(_, r)| matches!(self.resources[r], ShaderResource::Input(_)))
        })
    }
}
impl From<ShaderPipeline> for crate::Filter {
    fn from(pipeline: ShaderPipeline) -> Self {
        Self::ShaderPipeline(pipeline)
    }
}
