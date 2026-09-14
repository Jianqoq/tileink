use super::WgpuSceneBuffers;
use crate::{
    Canvas,
    render::{resource_writes::ReadyResource, vector_images::image_copy_regions},
    shared::image_resource::{GpuImageResourceUpload, ImageResourcePlacement},
    wgpu::commands::WgpuCommandBatch,
};
use std::rc::Weak;

pub(crate) struct VectorImageRequest {
    pub(crate) source: Weak<Canvas>,
    pub(crate) placement: ImageResourcePlacement,
}

pub(crate) struct VectorImageUpload {
    pub(crate) requests: Vec<VectorImageRequest>,
    pub(crate) ready: ReadyResource,
}

impl WgpuSceneBuffers {
    pub(super) fn prepare_vector_image_upload(
        &mut self,
        upload: &GpuImageResourceUpload,
        force_all: bool,
    ) {
        let retry = self
            .vector_image_upload
            .as_ref()
            .is_some_and(|pending| !pending.ready.is_submitted());
        let requests: Vec<_> = upload
            .vectors()
            .iter()
            .filter(|image| force_all || retry || image.dirty)
            .map(|image| VectorImageRequest {
                // Quarantined/local resource sets may outlive the source graph. Their
                // pending copies must not retain detached Canvas payloads.
                source: std::rc::Rc::downgrade(&image.canvas),
                placement: image.placement,
            })
            .collect();
        self.vector_image_upload = (!requests.is_empty()).then(|| VectorImageUpload {
            requests,
            ready: ReadyResource::pending(),
        });
    }

    pub(crate) fn vector_image_upload(&self) -> Option<&VectorImageUpload> {
        self.vector_image_upload.as_ref()
    }

    pub(crate) fn copy_vector_image(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::Texture,
        placement: ImageResourcePlacement,
    ) {
        let destination = match placement {
            ImageResourcePlacement::Atlas(_) => &self.image_resource_atlas,
            ImageResourcePlacement::Texture(rect) => {
                &self.image_resource_textures[rect.index as usize]
            }
        };
        for copy in image_copy_regions(placement) {
            commands.encoder().copy_texture_to_texture(
                ::wgpu::TexelCopyTextureInfo {
                    texture: source,
                    mip_level: 0,
                    origin: ::wgpu::Origin3d {
                        x: copy.source[0],
                        y: copy.source[1],
                        z: 0,
                    },
                    aspect: ::wgpu::TextureAspect::All,
                },
                ::wgpu::TexelCopyTextureInfo {
                    texture: destination,
                    mip_level: 0,
                    origin: ::wgpu::Origin3d {
                        x: copy.destination[0],
                        y: copy.destination[1],
                        z: copy.destination[2],
                    },
                    aspect: ::wgpu::TextureAspect::All,
                },
                ::wgpu::Extent3d {
                    width: copy.extent[0],
                    height: copy.extent[1],
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}
