use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::shared::image::Image;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImageKey(pub u64);

impl ImageKey {
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl From<u64> for ImageKey {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ImageResourceStore {
    images: FxHashMap<ImageKey, Arc<Image>>,
}

impl ImageResourceStore {
    pub(crate) fn insert(&mut self, key: ImageKey, image: impl Into<Arc<Image>>) -> bool {
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return false;
        }
        self.images.insert(key, image);
        true
    }

    pub(crate) fn get(&self, key: ImageKey) -> Option<&Image> {
        self.images.get(&key).map(Arc::as_ref)
    }

    pub(crate) fn upload(&self) -> GpuImageResourceUpload {
        let mut upload = GpuImageResourceUpload::default();
        for (&key, image) in &self.images {
            let index = upload.metadata.len() as u32 / GPU_IMAGE_RESOURCE_METADATA_STRIDE as u32;
            let pixel_offset = upload.pixels.len() as u32;
            upload.metadata.extend_from_slice(&[
                pixel_offset,
                image.pixels.len() as u32,
                image.width,
                image.height,
            ]);
            upload.pixels.extend_from_slice(&image.pixels);
            upload.indices.insert(key, index);
        }
        upload
    }
}

pub(crate) const GPU_IMAGE_RESOURCE_METADATA_STRIDE: usize = 4;

#[derive(Clone, Default)]
pub(crate) struct GpuImageResourceUpload {
    pub(crate) metadata: Vec<u32>,
    pub(crate) pixels: Vec<u32>,
    indices: FxHashMap<ImageKey, u32>,
}

impl GpuImageResourceUpload {
    #[inline]
    pub(crate) fn image_index(&self, key: ImageKey) -> Option<u32> {
        self.indices.get(&key).copied()
    }
}
