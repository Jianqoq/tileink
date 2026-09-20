use super::{Params, Probe, Result};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::NSString;
use objc2_metal::*;
use std::{collections::BTreeMap, ptr::NonNull};

pub struct Native {
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipelines: BTreeMap<&'static str, Retained<ProtocolObject<dyn MTLComputePipelineState>>>,
}

impl Native {
    #[allow(deprecated)]
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let device = MTLCreateSystemDefaultDevice().ok_or("Metal GPU unavailable")?;
        let queue = device.newCommandQueue().ok_or("Metal queue unavailable")?;
        let library =
            device.newLibraryWithData_error(&dispatch2::DispatchData::from_bytes(bytes))?;
        let mut pipelines = BTreeMap::new();
        for entry in ["clear_words", "copy_words", "layout_words", "sample_words"] {
            let function = library
                .newFunctionWithName(&NSString::from_str(entry))
                .ok_or("missing Metal entry")?;
            let mut reflection = None;
            // SAFETY: these are validated probe kernels; all dispatch inputs go
            // through Probe::validate and are retained by the command buffer.
            let pipeline = unsafe {
                device.newComputePipelineStateWithFunction_options_reflection_error(
                    &function,
                    MTLPipelineOption::ArgumentInfo | MTLPipelineOption::BufferTypeInfo,
                    Some(&mut reflection),
                )?
            };
            validate_reflection(
                reflection
                    .as_deref()
                    .ok_or("Metal reflection unavailable")?,
                entry,
            )?;
            if pipeline.maxTotalThreadsPerThreadgroup() < 64 {
                return Err("Metal probe requires 64 threads per group".into());
            }
            pipelines.insert(entry, pipeline);
        }
        Ok(Self {
            device,
            queue,
            pipelines,
        })
    }

    fn buffer(&self, bytes: &[u8]) -> Result<Retained<ProtocolObject<dyn MTLBuffer>>> {
        // SAFETY: Metal copies exactly bytes.len() initialized bytes immediately.
        unsafe {
            self.device.newBufferWithBytes_length_options(
                NonNull::new(bytes.as_ptr().cast_mut().cast()).unwrap(),
                bytes.len(),
                MTLResourceOptions::StorageModeShared,
            )
        }
        .ok_or_else(|| "Metal buffer allocation failed".into())
    }

    pub fn execute(&self, case: &Probe) -> Result<Vec<u8>> {
        case.validate()?;
        let destination = self.buffer(&case.destination)?;
        let source = self.buffer(&case.source)?;
        // A nonzero aligned offset checks that argument offsets participate in ABI.
        let mut uniforms = vec![0xcc; 256 + std::mem::size_of::<Params>()];
        uniforms[256..].copy_from_slice(bytemuck::bytes_of(&case.params));
        let params = self.buffer(&uniforms)?;
        let width = if case.entry == "sample_words" {
            case.params.value[2] as usize
        } else {
            1
        };
        // SAFETY: a supported RGBA8 format and validated, nonzero extent.
        let descriptor = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::RGBA8Unorm,
                width,
                1,
                false,
            )
        };
        descriptor.setStorageMode(MTLStorageMode::Shared);
        descriptor.setUsage(MTLTextureUsage::ShaderRead);
        let texture = self
            .device
            .newTextureWithDescriptor(&descriptor)
            .ok_or("Metal texture allocation failed")?;
        let start = if case.entry == "sample_words" {
            case.params.source_offset as usize
        } else {
            0
        };
        // SAFETY: validated source covers width RGBA8 texels; private to this work.
        unsafe {
            texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width,
                        height: 1,
                        depth: 1,
                    },
                },
                0,
                NonNull::new(case.source[start..].as_ptr().cast_mut().cast()).unwrap(),
                width * 4,
            );
        }
        let command = self
            .queue
            .commandBuffer()
            .ok_or("Metal command buffer failed")?;
        let encoder = command
            .computeCommandEncoder()
            .ok_or("Metal encoder failed")?;
        encoder.setComputePipelineState(&self.pipelines[case.entry]);
        // SAFETY: binding types, lengths, workgroup tails and CPU/GPU ownership
        // are checked above; no CPU access occurs before command completion.
        unsafe {
            encoder.setBuffer_offset_atIndex(Some(&destination), 0, 0);
            encoder.setBuffer_offset_atIndex(Some(&source), 0, 1);
            encoder.setBuffer_offset_atIndex(Some(&params), 256, 2);
            encoder.setTexture_atIndex(Some(&texture), 3);
        }
        encoder.dispatchThreadgroups_threadsPerThreadgroup(
            MTLSize {
                width: case.params.count.div_ceil(64).max(1) as usize,
                height: 1,
                depth: 1,
            },
            MTLSize {
                width: 64,
                height: 1,
                depth: 1,
            },
        );
        encoder.endEncoding();
        command.commit();
        command.waitUntilCompleted();
        if command.status() != MTLCommandBufferStatus::Completed {
            return Err(format!("Metal execution failed: {:?}", command.error()).into());
        }
        // SAFETY: completed shared storage, initialized allocation of exact size.
        Ok(unsafe {
            std::slice::from_raw_parts(
                destination.contents().as_ptr().cast::<u8>(),
                case.destination.len(),
            )
        }
        .to_vec())
    }
}

#[allow(deprecated)]
fn validate_reflection(reflection: &MTLComputePipelineReflection, entry: &str) -> Result<()> {
    use super::abi::{Kind, Scalar};
    let interface = super::interfaces::get("probe")?;
    let expected = interface.resources_for(entry)?;
    let arguments: Vec<_> = reflection
        .arguments()
        .iter()
        .filter(|a| a.isActive())
        .collect();
    if arguments.len() != expected.len() {
        return Err("Metal binding count mismatch".into());
    }
    for (name, resource) in expected {
        let kind = match resource.kind {
            Kind::Texture => MTLArgumentType::Texture,
            Kind::Read | Kind::Write | Kind::Uniform => MTLArgumentType::Buffer,
            _ => return Err("unsupported Metal probe resource kind".into()),
        };
        let argument = arguments
            .iter()
            .find(|a| a.r#type() == kind && a.index() == resource.binding as usize)
            .ok_or("missing Metal binding")?;
        let access = if resource.kind == Kind::Write {
            MTLBindingAccess::ReadWrite
        } else {
            MTLBindingAccess::ReadOnly
        };
        if argument.access() != access {
            return Err(format!("Metal {name} access mismatch").into());
        }
        if kind == MTLArgumentType::Texture {
            if argument.textureType() != MTLTextureType::Type2D
                || argument.textureDataType() != MTLDataType::Float
            {
                return Err("Metal texture type mismatch".into());
            }
            continue;
        }
        let alignment = if resource.kind == Kind::Uniform {
            16
        } else {
            4
        };
        if argument.bufferDataSize() != resource.size as usize
            || argument.bufferAlignment() != alignment
        {
            return Err(format!("Metal {name} size/alignment mismatch").into());
        }
        if resource.kind != Kind::Uniform {
            if argument.bufferDataType() != MTLDataType::UInt {
                return Err("Metal word buffer type mismatch".into());
            }
            continue;
        }
        let structure = argument
            .bufferStructType()
            .ok_or("missing Metal parameter structure")?;
        let members = structure.members();
        if members.len() != resource.fields.len() {
            return Err("Metal struct member count mismatch".into());
        }
        for (member, field) in members.iter().zip(&resource.fields) {
            let kind = match (field.scalar, field.lanes) {
                (Scalar::U32, 1) => MTLDataType::UInt,
                (Scalar::U32, 4) => MTLDataType::UInt4,
                _ => return Err("unsupported Metal probe field type".into()),
            };
            if member.name().to_string() != field.name
                || member.offset() != field.offset as usize
                || member.dataType() != kind
            {
                return Err(format!("Metal {name}.{} layout mismatch", field.name).into());
            }
        }
    }
    Ok(())
}
