use super::*;
use crate::native::{
    NativeContext, NativeTexture,
    interop::metal::{ContextDescriptor, TextureDescriptor},
};
use objc2_metal::*;

impl Adapter {
    pub fn from_metal(descriptor: ContextDescriptor) -> Result<Self> {
        let device = super::super::metal::Metal::from_objects(descriptor.device, descriptor.queue)?;
        Ok(Self(Rc::new(RefCell::new(Device::Metal(Box::new(device))))))
    }
    pub fn import_metal_texture(
        &self,
        context: &NativeContext,
        descriptor: TextureDescriptor,
    ) -> Result<NativeTexture> {
        let Device::Metal(device) = &*self.0.borrow();
        let texture = descriptor.texture;
        if !std::ptr::eq(&*texture.device(), &*device.device)
            || texture.pixelFormat() != MTLPixelFormat::RGBA8Unorm
            || texture.sampleCount() != 1
            || texture.mipmapLevelCount() != 1
            || !matches!(
                texture.textureType(),
                MTLTextureType::Type2D | MTLTextureType::Type2DArray
            )
            || !texture
                .usage()
                .contains(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite)
            || texture.hazardTrackingMode() == MTLHazardTrackingMode::Untracked
            || texture.width() == 0
            || texture.width() > 16384
            || texture.height() == 0
            || texture.height() > 16384
            || texture.arrayLength() == 0
            || texture.arrayLength() > 2048
        {
            return Err(
                "incompatible Metal texture device/format/usage/dimensions/hazard tracking".into(),
            );
        }
        let size = [texture.width() as u32, texture.height() as u32];
        let layers = texture.arrayLength() as u32;
        let array = texture.textureType() == MTLTextureType::Type2DArray;
        Ok(NativeTexture {
            context: context.clone(),
            size,
            layers,
            array,
            state: Rc::new(super::super::texture::State {
                allocation: super::super::texture::Allocation::Metal(texture),
                initialized: std::cell::Cell::new(descriptor.initialized),
                content_version: std::cell::Cell::new(0),
            }),
        })
    }
}
