//! Public zero-copy native invocations for the same pipeline interface as filters.
use super::runtime::compute::{ComputeBatch, ResourceId, SamplerFilter, SurfacePool};
use super::{NativeContext, NativeError, NativeSubmission, NativeTexture};
use crate::{ShaderPipeline, ShaderSampler};
use std::{cell::RefCell, rc::Rc};

/// Opaque batch-owned GPU resource. Foreign-batch handles are rejected at record
/// time; command submissions retain allocations until completion.
#[derive(Clone, Copy, Debug)]
pub struct NativeShaderResource(pub(crate) ResourceId);

/// Explicit native resource invocation.
///
/// Use External descriptors with these
/// handles for buffers, arbitrary uniforms, arrays and descriptor tables. Canvas
/// filters instead resolve Input/Output/Config automatically. Both use exactly
/// the same `ShaderPipeline` descriptors, validator and pass executor.
pub struct NativeShaderBatch {
    context: NativeContext,
    pub(crate) batch: ComputeBatch,
}
impl NativeShaderBatch {
    #[must_use]
    pub fn new(context: &NativeContext) -> Self {
        Self {
            context: context.clone(),
            batch: ComputeBatch::with_surfaces(Rc::new(RefCell::new(SurfacePool::new(context)))),
        }
    }
    ///
    /// # Errors
    /// Returns an error if the buffer byte extent is invalid or exceeds native addressing limits.
    pub fn buffer(&mut self, bytes: Vec<u8>) -> Result<NativeShaderResource, NativeError> {
        self.batch
            .buffer(bytes)
            .map(NativeShaderResource)
            .map_err(NativeError::Recording)
    }
    ///
    /// # Errors
    /// Returns an error if the dimensions or RGBA8 byte count are invalid.
    pub fn texture_rgba8(
        &mut self,
        size: [u32; 2],
        bytes: Vec<u8>,
    ) -> Result<NativeShaderResource, NativeError> {
        self.batch
            .texture_rgba8(size, bytes)
            .map(NativeShaderResource)
            .map_err(NativeError::Recording)
    }
    ///
    /// # Errors
    /// Returns an error if the dimensions, layer count, or RGBA8 byte count are invalid.
    pub fn texture_array_rgba8(
        &mut self,
        size: [u32; 3],
        bytes: Vec<u8>,
    ) -> Result<NativeShaderResource, NativeError> {
        self.batch
            .texture_array_rgba8(size, bytes)
            .map(NativeShaderResource)
            .map_err(NativeError::Recording)
    }
    /// Import a persistent image without uploading or copying its pixels.
    ///
    /// # Errors
    /// Returns an error if the texture belongs to another device or its import contract is invalid.
    pub fn import_texture(
        &mut self,
        texture: &NativeTexture,
    ) -> Result<NativeShaderResource, NativeError> {
        if !self.context.adapter.same_device(&texture.context.adapter) {
            return Err(NativeError::Recording(
                "shader texture belongs to another device".into(),
            ));
        }
        self.batch
            .import_texture(texture)
            .map(NativeShaderResource)
            .map_err(NativeError::Recording)
    }
    ///
    /// # Errors
    /// Returns an error for invalid/foreign handles, incompatible texture kinds, or an unsupported table length.
    pub fn texture_table(
        &mut self,
        images: &[NativeShaderResource],
    ) -> Result<NativeShaderResource, NativeError> {
        self.batch
            .texture_table(&images.iter().map(|r| r.0).collect::<Vec<_>>())
            .map(NativeShaderResource)
            .map_err(NativeError::Recording)
    }
    /// Record an immutable descriptor. Native allocation errors are returned by submit.
    pub fn sampler(&mut self, filter: ShaderSampler) -> NativeShaderResource {
        NativeShaderResource(self.batch.sampler(match filter {
            ShaderSampler::Nearest => SamplerFilter::Nearest,
            ShaderSampler::Linear => SamplerFilter::Linear,
        }))
    }
    ///
    /// # Errors
    /// Returns an error for missing/foreign resources, binding/type mismatches, invalid extents, or uninitialized reads.
    pub fn record(
        &mut self,
        pipeline: &ShaderPipeline,
        resources: &[NativeShaderResource],
    ) -> Result<(), NativeError> {
        super::runtime::program_shader::pipeline::record_bindings(
            &mut self.batch,
            pipeline,
            &resources.iter().map(|r| r.0).collect::<Vec<_>>(),
        )
        .map_err(NativeError::Recording)
    }
    /// Submit ordered passes without CPU waits or readbacks between passes.
    ///
    /// # Errors
    /// Returns an error if native allocation, encoding, or queue submission fails; unconfirmed submissions are reported separately.
    pub fn submit(self) -> Result<NativeSubmission, NativeError> {
        self.context.submit_compute(&self.batch)
    }
}
