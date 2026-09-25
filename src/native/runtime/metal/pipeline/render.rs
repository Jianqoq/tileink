//! Render pipeline creation and validation for the analytic fine draw.
use super::*;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;

#[allow(deprecated)]
pub(super) fn create(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    tile: &ProtocolObject<dyn MTLFunction>,
    shader: &NativeShaderArtifact,
) -> Result<(State, Vec<Retained<MTLArgument>>)> {
    let vertex = library
        .newFunctionWithName(&NSString::from_str("fine_tile_vertex"))
        .ok_or("Metal fine vertex missing")?;
    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setVertexFunction(Some(&vertex));
    let fragment = library
        .newFunctionWithName(&NSString::from_str("fine_tile_sparse"))
        .ok_or("Metal sparse fragment missing")?;
    descriptor.setFragmentFunction(Some(&fragment));
    // SAFETY: attachment zero is within the eight Metal color slots.
    unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
        .setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    let mut reflection = None;
    let sparse = device.newRenderPipelineStateWithDescriptor_options_reflection_error(
        &descriptor,
        MTLPipelineOption::ArgumentInfo | MTLPipelineOption::BufferTypeInfo,
        Some(&mut reflection),
    )?;
    let reflection = reflection.ok_or("Metal render reflection unavailable")?;
    let vertex_arguments = reflection
        .vertexArguments()
        .ok_or("Metal vertex reflection missing")?;
    let active: Vec<_> = vertex_arguments.iter().filter(|a| a.isActive()).collect();
    if active.len() != 2
        || active.iter().any(|a| a.r#type() != MTLArgumentType::Buffer)
        || !active.iter().any(|a| a.index() == 4)
    {
        return Err("Metal fine vertex bindings mismatch".into());
    }
    let config = active
        .iter()
        .find(|a| a.index() == 0)
        .ok_or("Metal vertex config missing")?;
    super::reflection::uniform(config, shader, 0)?;
    let (full, arguments) = tile_pipeline(device, tile)?;
    let sparse_arguments: Vec<_> = reflection
        .fragmentArguments()
        .ok_or("Metal sparse fragment reflection missing")?
        .iter()
        .filter(|a| a.isActive())
        .collect();
    if sparse_arguments.len() != arguments.len()
        || sparse_arguments.iter().any(|a| {
            !arguments
                .iter()
                .any(|b| a.index() == b.index() && a.r#type() == b.r#type())
        })
    {
        return Err("Metal sparse fragment bindings differ from full drawing".into());
    }
    for binding in shader
        .bindings
        .iter()
        .filter(|b| b.kind == BindingKind::Uniform)
    {
        let argument = sparse_arguments
            .iter()
            .find(|a| a.r#type() == MTLArgumentType::Buffer && a.index() == slot(binding.slot))
            .ok_or("Metal sparse uniform missing")?;
        super::reflection::uniform(argument, shader, binding.slot)?;
    }
    Ok((State::Tile { full, sparse }, arguments))
}

#[allow(deprecated)]
type ReflectedTile = (
    Object<dyn MTLRenderPipelineState>,
    Vec<Retained<MTLArgument>>,
);

#[allow(deprecated)]
fn tile_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    function: &ProtocolObject<dyn MTLFunction>,
) -> Result<ReflectedTile> {
    let descriptor = MTLTileRenderPipelineDescriptor::new();
    // SAFETY: the tile ABI is reflected before resources are bound or dispatched.
    unsafe {
        descriptor.setTileFunction(function);
        descriptor.setRasterSampleCount(1);
        descriptor
            .colorAttachments()
            .objectAtIndexedSubscript(0)
            .setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    }
    descriptor.setThreadgroupSizeMatchesTileSize(true);
    descriptor.setMaxTotalThreadsPerThreadgroup(256);
    let mut reflection = None;
    let state = device.newRenderPipelineStateWithTileDescriptor_options_reflection_error(
        &descriptor,
        MTLPipelineOption::ArgumentInfo | MTLPipelineOption::BufferTypeInfo,
        Some(&mut reflection),
    )?;
    if state.maxTotalThreadsPerThreadgroup() < 256 {
        return Err("Metal tile kernel exceeds workgroup limit".into());
    }
    let arguments: Vec<_> = reflection
        .ok_or("Metal tile reflection unavailable")?
        .tileArguments()
        .ok_or("Metal tile arguments missing")?
        .iter()
        .filter(|a| a.isActive())
        .collect();
    if arguments
        .iter()
        .filter(|a| a.r#type() == MTLArgumentType::Imageblock)
        .count()
        != 1
    {
        return Err("Metal fine shader requires one implicit imageblock".into());
    }
    Ok((
        state,
        arguments
            .into_iter()
            .filter(|a| a.r#type() != MTLArgumentType::Imageblock)
            .collect(),
    ))
}
