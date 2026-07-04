use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::shared::{gpu_layout::image_resource::GPU_IMAGE_RESOURCE_METADATA_STRIDE, image::Image};

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

    pub(crate) fn remove(&mut self, key: ImageKey) -> bool {
        self.images.remove(&key).is_some()
    }

    pub(crate) fn clear(&mut self) -> bool {
        let had_images = !self.images.is_empty();
        self.images.clear();
        had_images
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

#[cfg(test)]
mod tests {
    use super::{ImageKey, ImageResourceStore};
    use crate::shared::image::Image;

    #[test]
    fn remove_and_clear_update_image_resource_uploads() {
        let mut resources = ImageResourceStore::default();
        let first = ImageKey::new(1);
        let second = ImageKey::new(2);

        assert!(resources.insert(first, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
        assert!(resources.insert(second, Image::from_rgba8(1, 1, [0, 255, 0, 255])));
        let upload = resources.upload();
        assert!(upload.image_index(first).is_some());
        assert!(upload.image_index(second).is_some());

        assert!(resources.remove(first));
        assert!(!resources.remove(first));
        let upload = resources.upload();
        assert!(upload.image_index(first).is_none());
        assert!(upload.image_index(second).is_some());

        assert!(resources.clear());
        assert!(!resources.clear());
        let upload = resources.upload();
        assert!(upload.metadata.is_empty());
        assert!(upload.pixels.is_empty());
        assert!(upload.image_index(second).is_none());
    }
}
