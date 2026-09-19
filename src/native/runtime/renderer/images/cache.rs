//! Immutable raster/vector image contents survive until the upload identity changes.
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
        if batch.context().is_none() {
            return Self::record_with_vectors(batch, upload, render);
        }
        if cache
            .as_ref()
            .is_none_or(|cached| cached.signature != signature || !cached.accepted.get())
        {
            let images = SceneImages::record_with_vectors(batch, upload, render)?;
            let atlas = batch.snapshot_texture(images.atlas)?;
            let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
                unreachable!("image table")
            };
            let table = table.clone();
            let mut snapshots = HashMap::new();
            let mut textures = Vec::with_capacity(table.len());
            for image in table {
                if let std::collections::hash_map::Entry::Vacant(entry) = snapshots.entry(image) {
                    entry.insert(batch.snapshot_texture(image)?);
                }
                textures.push(snapshots[&image].clone());
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
