//! Host presentation shader only; Tileink's rendering kernels are offline MSL.
use super::*;
use objc2_foundation::NSString;
pub(super) fn pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Object<dyn MTLRenderPipelineState>> {
    let library = device.newLibraryWithSource_options_error(
        &NSString::from_str(include_str!("present.metal")),
        None,
    )?;
    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setVertexFunction(Some(
        &*library
            .newFunctionWithName(&NSString::from_str("present_vertex"))
            .ok_or("present vertex")?,
    ));
    descriptor.setFragmentFunction(Some(
        &*library
            .newFunctionWithName(&NSString::from_str("present_fragment"))
            .ok_or("present fragment")?,
    ));
    // SAFETY: attachment zero is within Metal's eight color slots.
    unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
        .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    Ok(device.newRenderPipelineStateWithDescriptor_error(&descriptor)?)
}
pub(super) fn encode(
    command: &ProtocolObject<dyn MTLCommandBuffer>,
    pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
    source: &ProtocolObject<dyn MTLTexture>,
    destination: &ProtocolObject<dyn MTLTexture>,
) -> Result {
    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: attachment zero is always a valid color slot.
    let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(destination));
    color.setLoadAction(MTLLoadAction::DontCare);
    color.setStoreAction(MTLStoreAction::Store);
    let encoder = command
        .renderCommandEncoderWithDescriptor(&descriptor)
        .ok_or("presentation render encoder")?;
    encoder.setRenderPipelineState(pipeline);
    // SAFETY: full-screen triangle covers the drawable; source has the same
    // extent, initialized pixels and ShaderRead usage; no vertex buffers needed.
    unsafe {
        encoder.setFragmentTexture_atIndex(Some(source), 0);
        encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
    }
    encoder.endEncoding();
    Ok(())
}
