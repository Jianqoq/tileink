//! A workload pass binds validated resources to its actual Metal stage.
use super::*;
use crate::native::runtime::compute::Pass;
use objc2::runtime::ProtocolObject;
use std::ptr::NonNull;

pub(super) enum PassEncoder {
    Compute(Object<dyn MTLComputeCommandEncoder>),
    Tile(Object<dyn MTLRenderCommandEncoder>),
    Sparse(Object<dyn MTLRenderCommandEncoder>, usize),
}

impl PassEncoder {
    pub fn new(
        command: &ProtocolObject<dyn MTLCommandBuffer>,
        state: &pipeline::State,
        batch: &ComputeBatch,
        pass: &Pass,
        resources: &[Resource],
    ) -> Result<Self> {
        match state {
            pipeline::State::Compute(state) => {
                let encoder = command
                    .computeCommandEncoder()
                    .ok_or("Metal compute encoder failed")?;
                encoder.setComputePipelineState(state);
                Ok(Self::Compute(encoder))
            }
            pipeline::State::Tile { full, sparse } => {
                let (descriptor, config) = super::render::descriptor(batch, pass, resources)?;
                if config.incremental == 0 {
                    descriptor.setImageblockSampleLength(full.imageblockSampleLength());
                }
                let encoder = command
                    .renderCommandEncoderWithDescriptor(&descriptor)
                    .ok_or("Metal render encoder failed")?;
                if config.incremental == 0 {
                    let result = Self::Tile(encoder);
                    let Self::Tile(encoder) = &result else {
                        unreachable!()
                    };
                    if encoder.tileWidth() != 16 || encoder.tileHeight() != 16 {
                        return Err(
                            "Metal requires 16x16 hardware tiles for analytic coverage".into()
                        );
                    }
                    encoder.setRenderPipelineState(full);
                    Ok(result)
                } else {
                    let result = Self::Sparse(encoder, config.active_tile_count as usize);
                    let Self::Sparse(encoder, _) = &result else {
                        unreachable!()
                    };
                    encoder.setRenderPipelineState(sparse);
                    encoder.setViewport(MTLViewport {
                        originX: 0.0,
                        originY: 0.0,
                        width: config.width as f64,
                        height: config.height as f64,
                        znear: 0.0,
                        zfar: 1.0,
                    });
                    // Clip partial edge tiles even when the backing target is larger.
                    encoder.setScissorRect(MTLScissorRect {
                        x: 0,
                        y: 0,
                        width: config.width as usize,
                        height: config.height as usize,
                    });
                    // SAFETY: reflected vertex inputs and unique active tile IDs.
                    unsafe {
                        for (binding, id) in &pass.bindings {
                            if matches!(binding.slot, 0 | 4) {
                                encoder.setVertexBuffer_offset_atIndex(
                                    Some(resources[id.index()].buffer()?),
                                    0,
                                    binding.slot as usize,
                                );
                            }
                        }
                    }
                    Ok(result)
                }
            }
        }
    }

    /// # Safety
    /// Frame recording must supply reflected slots and owned, bounded resources.
    pub unsafe fn buffer(&self, buffer: &ProtocolObject<dyn MTLBuffer>, slot: usize) {
        unsafe {
            match self {
                Self::Compute(e) => e.setBuffer_offset_atIndex(Some(buffer), 0, slot),
                Self::Tile(e) => e.setTileBuffer_offset_atIndex(Some(buffer), 0, slot),
                Self::Sparse(e, _) => e.setFragmentBuffer_offset_atIndex(Some(buffer), 0, slot),
            }
        }
    }
    /// Bind a short-lived backend constant without allocating an MTLBuffer for
    /// every pass. Metal copies the data before this call returns.
    ///
    /// # Safety
    /// Bytes must match the reflected slot and be smaller than 4 KiB.
    pub unsafe fn bytes(&self, bytes: &[u8], slot: usize) {
        debug_assert!(!bytes.is_empty() && bytes.len() < 4096);
        let pointer = NonNull::new(bytes.as_ptr().cast_mut().cast()).unwrap();
        unsafe {
            match self {
                Self::Compute(e) => e.setBytes_length_atIndex(pointer, bytes.len(), slot),
                Self::Tile(e) => e.setTileBytes_length_atIndex(pointer, bytes.len(), slot),
                Self::Sparse(e, _) => e.setFragmentBytes_length_atIndex(pointer, bytes.len(), slot),
            }
        }
    }
    /// # Safety
    /// The texture must match the reflected slot, usage, and dimensionality.
    pub unsafe fn texture(&self, texture: &ProtocolObject<dyn MTLTexture>, slot: usize) {
        unsafe {
            match self {
                Self::Compute(e) => e.setTexture_atIndex(Some(texture), slot),
                // Slot one is the color attachment, never a sampled read/write alias.
                Self::Tile(e) if slot != 1 => e.setTileTexture_atIndex(Some(texture), slot),
                Self::Sparse(e, _) if slot != 1 => {
                    e.setFragmentTexture_atIndex(Some(texture), slot)
                }
                _ => {}
            }
        }
    }
    /// # Safety
    /// The sampler must belong to this device and the slot must be reflected.
    pub unsafe fn sampler(&self, sampler: &ProtocolObject<dyn MTLSamplerState>, slot: usize) {
        unsafe {
            match self {
                Self::Compute(e) => e.setSamplerState_atIndex(Some(sampler), slot),
                Self::Tile(e) => e.setTileSamplerState_atIndex(Some(sampler), slot),
                Self::Sparse(e, _) => e.setFragmentSamplerState_atIndex(Some(sampler), slot),
            }
        }
    }
    pub fn sampled_resource(&self, resource: &ProtocolObject<dyn MTLResource>) {
        match self {
            Self::Compute(e) => e.useResource_usage(resource, MTLResourceUsage::Read),
            Self::Tile(e) => {
                e.useResource_usage_stages(resource, MTLResourceUsage::Read, MTLRenderStages::Tile)
            }
            Self::Sparse(e, _) => e.useResource_usage_stages(
                resource,
                MTLResourceUsage::Read,
                MTLRenderStages::Fragment,
            ),
        }
    }
    pub fn execute(&self, pass: &Pass) {
        match self {
            Self::Compute(e) => e.dispatchThreadgroups_threadsPerThreadgroup(
                memory::size(pass.grid),
                memory::size(pass.shader.workgroup),
            ),
            Self::Tile(e) => e.dispatchThreadsPerTile(memory::size([16, 16, 1])),
            Self::Sparse(e, count) => {
                // SAFETY: the validated active list has one unique in-bounds tile per instance.
                unsafe {
                    e.drawPrimitives_vertexStart_vertexCount_instanceCount(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        *count,
                    )
                };
            }
        }
    }
}
impl Drop for PassEncoder {
    fn drop(&mut self) {
        match self {
            Self::Compute(e) => e.endEncoding(),
            Self::Tile(e) | Self::Sparse(e, _) => e.endEncoding(),
        }
    }
}
