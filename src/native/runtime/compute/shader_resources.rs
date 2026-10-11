//! Exact-size shader leases. Format is part of allocation identity; immutable
//! snapshots are uploaded once, while writable buffers reset on each invocation.
use super::{ComputeBatch, Resource, ResourceId, Texture};
use crate::{
    ShaderTextureFormat,
    native::{
        NativeContext, NativeTexture,
        runtime::{Result, buffer::Buffer},
    },
};
use std::rc::{Rc, Weak};

#[derive(Default)]
pub(super) struct ShaderResourcePool {
    textures: Vec<NativeTexture>,
    buffers: Vec<Buffer>,
    images: Vec<(Weak<[u8]>, NativeTexture)>,
}
impl ShaderResourcePool {
    pub(super) fn begin_frame(&mut self) {
        self.images.retain(|(pixels, _)| pixels.strong_count() != 0);
        // Keep peak steady-state leases, but bound idle capacity after animated
        // sizes/formats disappear. Submitted batches retain their own owners.
        if self.textures.len() > 128 {
            self.textures.retain(|t| Rc::strong_count(&t.state) > 1);
        }
        if self.buffers.len() > 128 {
            self.buffers.retain(|b| Rc::strong_count(&b.state) > 1);
        }
    }
    fn texture(
        &mut self,
        context: &NativeContext,
        size: [u32; 2],
        format: ShaderTextureFormat,
    ) -> Result<NativeTexture> {
        if let Some(t) = self
            .textures
            .iter()
            .find(|t| t.size == size && t.format == format && Rc::strong_count(&t.state) == 1)
        {
            return Ok(t.clone());
        }
        let t = context.create_texture_format(size, 1, false, format)?;
        self.textures.push(t.clone());
        Ok(t)
    }
    fn buffer(&mut self, context: &NativeContext, size: usize) -> Result<Buffer> {
        if let Some(b) = self
            .buffers
            .iter()
            .find(|b| b.state.size == size && Rc::strong_count(&b.state) == 1)
        {
            return Ok(b.clone());
        }
        let b = Buffer::new(&context.adapter, size)?;
        self.buffers.push(b.clone());
        Ok(b)
    }
    fn image(
        &mut self,
        context: &NativeContext,
        size: [u32; 2],
        format: ShaderTextureFormat,
        bytes: &Rc<[u8]>,
    ) -> Result<NativeTexture> {
        if let Some((_, t)) = self.images.iter().find(|(key, t)| {
            key.ptr_eq(&Rc::downgrade(bytes)) && t.size == size && t.format == format
        }) {
            return Ok(t.clone());
        }
        let t = context.create_texture_format(size, 1, false, format)?;
        self.images.push((Rc::downgrade(bytes), t.clone()));
        Ok(t)
    }
}
impl ComputeBatch {
    pub(crate) fn shader_texture(
        &mut self,
        size: [u32; 2],
        format: ShaderTextureFormat,
    ) -> Result<ResourceId> {
        let pool = self
            .surface_pool
            .as_ref()
            .ok_or("shader pipelines require a renderer resource pool")?;
        let texture = {
            let mut p = pool.borrow_mut();
            let context = p.context.clone();
            p.shader_resources.texture(&context, size, format)?
        };
        self.import_texture_inner(&texture, false)
    }
    pub(crate) fn shader_image(
        &mut self,
        size: [u32; 2],
        format: ShaderTextureFormat,
        bytes: &Rc<[u8]>,
    ) -> Result<ResourceId> {
        let pool = self
            .surface_pool
            .as_ref()
            .ok_or("shader pipelines require a renderer resource pool")?;
        let texture = {
            let mut p = pool.borrow_mut();
            let context = p.context.clone();
            p.shader_resources.image(&context, size, format, bytes)?
        };
        let id = self.import_texture_inner(&texture, false)?;
        if !texture.state.initialized.get() {
            let Resource::Texture(Texture { bytes: upload, .. }) = &mut self.resources[id.index()]
            else {
                unreachable!()
            };
            *upload = bytes.to_vec().into();
        }
        Ok(id)
    }
    pub(crate) fn shader_buffer(&mut self, words: &[u32]) -> Result<ResourceId> {
        let pool = self
            .surface_pool
            .as_ref()
            .ok_or("shader pipelines require a renderer resource pool")?;
        let buffer = {
            let mut p = pool.borrow_mut();
            let context = p.context.clone();
            p.shader_resources.buffer(&context, words.len() * 4)?
        };
        self.import_buffer(&buffer, &[(0, bytemuck::cast_slice(words))])
    }
}
