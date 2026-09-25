use super::{Object, Result};
use objc2_metal::*;
use std::ptr::NonNull;

pub(super) fn buffer(
    device: &ProtocolDevice,
    size: usize,
    options: MTLResourceOptions,
) -> Result<Object<dyn MTLBuffer>> {
    if size == 0 || size > device.maxBufferLength() {
        return Err("Metal buffer size exceeds device limit".into());
    }
    device
        .newBufferWithLength_options(
            size,
            options | MTLResourceOptions::HazardTrackingModeTracked,
        )
        .ok_or_else(|| "Metal buffer allocation failed".into())
}
type ProtocolDevice = objc2::runtime::ProtocolObject<dyn MTLDevice>;
pub(super) fn upload(device: &ProtocolDevice, bytes: &[u8]) -> Result<Object<dyn MTLBuffer>> {
    if bytes.is_empty() || bytes.len() > device.maxBufferLength() {
        return Err("Metal upload exceeds device limit".into());
    }
    // SAFETY: the input slice is initialized and copied synchronously by Metal.
    unsafe {
        device.newBufferWithBytes_length_options(
            NonNull::new(bytes.as_ptr().cast_mut().cast()).unwrap(),
            bytes.len(),
            MTLResourceOptions::StorageModeShared,
        )
    }
    .ok_or_else(|| "Metal staging allocation failed".into())
}
pub(super) fn texture(
    device: &ProtocolDevice,
    size: [u32; 2],
    layers: u32,
    array: bool,
) -> Result<Object<dyn MTLTexture>> {
    if size.contains(&0)
        || size.iter().any(|&n| n > 16384)
        || layers == 0
        || layers > 2048
        || (!array && layers != 1)
    {
        return Err("Metal texture dimensions exceed supported limits".into());
    }
    let descriptor = MTLTextureDescriptor::new();
    descriptor.setTextureType(if array {
        MTLTextureType::Type2DArray
    } else {
        MTLTextureType::Type2D
    });
    descriptor.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    // SAFETY: dimensions were checked against the supported Metal family limits.
    unsafe {
        descriptor.setWidth(size[0] as usize);
        descriptor.setHeight(size[1] as usize);
        descriptor.setArrayLength(layers as usize);
    }
    descriptor.setStorageMode(MTLStorageMode::Private);
    descriptor.setUsage(
        MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite | MTLTextureUsage::RenderTarget,
    );
    device
        .newTextureWithDescriptor(&descriptor)
        .ok_or_else(|| "Metal texture allocation failed".into())
}
pub(super) fn size(value: [u32; 3]) -> MTLSize {
    MTLSize {
        width: value[0] as usize,
        height: value[1] as usize,
        depth: value[2] as usize,
    }
}
pub(super) fn origin(value: [u32; 2]) -> MTLOrigin {
    MTLOrigin {
        x: value[0] as usize,
        y: value[1] as usize,
        z: 0,
    }
}
