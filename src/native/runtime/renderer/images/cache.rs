//! Immutable raster/vector image contents survive until the upload identity changes.
//! Cache recording requires native storage; it never bypasses retention by
//! selecting an uncached CPU upload path when the renderer pool is missing.
use super::{Images, SceneImages};
use crate::Canvas;
use crate::native::{
    NativeTexture,
    runtime::{
        Result,
        compute::{ComputeBatch, Resource, SamplerFilter},
    },
};
use crate::shared::image_resource::{GpuImageResourceUpload, ImageResourceUploadSignature};
use std::{cell::Cell, collections::HashMap, rc::Rc};

pub(crate) struct ImageCache {
    signature: ImageResourceUploadSignature,
    atlas: NativeTexture,
    textures: Vec<NativeTexture>,
    accepted: Rc<Cell<bool>>,
}
impl<'a> Images<'a> {
    pub(crate) fn record_cached(
        batch: &mut ComputeBatch,
        upload: &'a GpuImageResourceUpload,
        cache: &mut Option<ImageCache>,
        signature: ImageResourceUploadSignature,
        render: impl FnMut(
            &mut ComputeBatch,
            &Rc<Canvas>,
        ) -> Result<crate::native::runtime::compute::ResourceId>,
    ) -> Result<Self> {
        batch.context()?;
        if cache
            .as_ref()
            .is_none_or(|cached| cached.signature != signature || !cached.accepted.get())
        {
            let images = SceneImages::record_with_vectors(batch, upload, render)?;
            let atlas = batch.retain_image(images.atlas)?;
            let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
                unreachable!("image table")
            };
            let table = table.clone();
            let mut retained = HashMap::new();
            let mut textures = Vec::with_capacity(table.len());
            for image in table {
                if let std::collections::hash_map::Entry::Vacant(entry) = retained.entry(image) {
                    entry.insert(batch.retain_image(image)?);
                }
                textures.push(retained[&image].clone());
            }
            *cache = Some(ImageCache {
                signature,
                atlas,
                textures,
                accepted: batch.acceptance(),
            });
        }
        let cached = cache.as_ref().unwrap();
        let atlas = batch.import_texture(&cached.atlas)?;
        let textures = cached
            .textures
            .iter()
            .map(|texture| batch.import_texture(texture))
            .collect::<Result<Vec<_>>>()?;
        let textures = SceneImages {
            atlas,
            table: batch.texture_table(&textures)?,
            sampler: batch.sampler(SamplerFilter::Linear)?,
        };
        Ok(Self { upload, textures })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pool_cannot_bypass_image_caching_or_record_partial_resources() {
        let resources = crate::shared::image_resource::ImageResourceStore::default();
        let signature = resources.upload_signature(&resources, 32, 4, 0);
        let mut batch = ComputeBatch::new();
        let mut cache = None;
        assert!(
            Images::record_cached(
                &mut batch,
                &Default::default(),
                &mut cache,
                signature,
                |_, _| panic!("empty vector resources"),
            )
            .is_err()
        );
        assert!(cache.is_none());
        assert!(batch.resources().is_empty());
        assert!(batch.commands().is_empty());
    }
}
