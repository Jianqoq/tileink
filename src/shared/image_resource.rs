//! Scoped image stores and stable upload identities.
mod atlas;
mod source;
mod upload;
pub(crate) use source::ImageSource;
pub(crate) use upload::{AtlasRect, GpuImageResourceUpload, ImageResourcePlacement};

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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ImageResourceId {
    Renderer(ImageKey),
    Scene(ImageKey),
}

impl ImageResourceId {
    #[inline]
    pub(crate) fn renderer(key: ImageKey) -> Self {
        Self::Renderer(key)
    }

    #[inline]
    pub(crate) fn scene(key: ImageKey) -> Self {
        Self::Scene(key)
    }

    pub(crate) fn encode(self) -> (u32, u32, u32) {
        match self {
            Self::Renderer(key) => encode_key(0, key),
            Self::Scene(key) => encode_key(1, key),
        }
    }

    pub(crate) fn decode(scope: u32, low: u32, high: u32) -> Option<Self> {
        let key = ImageKey(low as u64 | ((high as u64) << 32));
        match scope {
            0 => Some(Self::Renderer(key)),
            1 => Some(Self::Scene(key)),
            _ => None,
        }
    }
}

fn encode_key(scope: u32, key: ImageKey) -> (u32, u32, u32) {
    (
        scope,
        key.0 as u32,
        ((key.0 >> 32) & u32::MAX as u64) as u32,
    )
}

#[derive(Clone, Default)]
pub(crate) struct ImageResourceStore {
    images: FxHashMap<ImageKey, ImageSource>,
    signature_hash: u64,
}

impl ImageResourceStore {
    pub(crate) fn insert(&mut self, key: ImageKey, image: impl Into<ImageSource>) -> bool {
        let image = image.into();
        if image.width() == 0 || image.height() == 0 {
            return false;
        }
        if let Some(previous) = self.images.insert(key, image.clone()) {
            self.signature_hash ^= image_entry_hash(key, &previous);
        }
        self.signature_hash ^= image_entry_hash(key, &image);
        true
    }

    pub(crate) fn contains(&self, key: ImageKey) -> bool {
        self.images.contains_key(&key)
    }

    pub(crate) fn get(&self, key: ImageKey) -> Option<&Image> {
        self.images.get(&key).and_then(ImageSource::raster)
    }

    pub(crate) fn remove(&mut self, key: ImageKey) -> bool {
        let Some(image) = self.images.remove(&key) else {
            return false;
        };
        self.signature_hash ^= image_entry_hash(key, &image);
        true
    }

    pub(crate) fn clear(&mut self) -> bool {
        let had_images = !self.images.is_empty();
        self.images.clear();
        self.signature_hash = 0;
        had_images
    }

    pub(crate) fn upload_merged(
        &self,
        scene: &ImageResourceStore,
        max_atlas_dimension: u32,
        max_atlas_pages: u32,
        texture_table_len: u32,
        previous: Option<&GpuImageResourceUpload>,
    ) -> GpuImageResourceUpload {
        GpuImageResourceUpload::from_stores(
            self,
            scene,
            max_atlas_dimension,
            max_atlas_pages,
            texture_table_len,
            previous,
        )
    }

    pub(crate) fn upload_signature(
        &self,
        scene: &ImageResourceStore,
        max_atlas_dimension: u32,
        max_atlas_pages: u32,
        texture_table_len: u32,
    ) -> ImageResourceUploadSignature {
        ImageResourceUploadSignature {
            renderer: self.store_signature(),
            scene: scene.store_signature(),
            max_atlas_dimension,
            max_atlas_pages,
            texture_table_len,
        }
    }

    pub(crate) fn extend_from(&mut self, other: &ImageResourceStore) {
        for (&key, image) in &other.images {
            self.insert(key, image.clone());
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (ImageKey, &ImageSource)> {
        self.images.iter().map(|(&key, image)| (key, image))
    }

    fn store_signature(&self) -> ImageResourceStoreSignature {
        ImageResourceStoreSignature {
            len: self.images.len() as u64,
            hash: self.signature_hash,
        }
    }
}

fn image_entry_hash(key: ImageKey, image: &ImageSource) -> u64 {
    let mut hash = FNV_OFFSET;
    hash = fnv_mix(hash, key.0);
    hash = fnv_mix(hash, image.width() as u64);
    hash = fnv_mix(hash, image.height() as u64);
    let (kind, identity) = image.identity();
    hash = fnv_mix(hash, kind);
    fnv_mix(hash, identity)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ImageResourceUploadSignature {
    renderer: ImageResourceStoreSignature,
    scene: ImageResourceStoreSignature,
    max_atlas_dimension: u32,
    max_atlas_pages: u32,
    texture_table_len: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ImageResourceStoreSignature {
    len: u64,
    hash: u64,
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[inline]
fn fnv_mix(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
pub(crate) const MAX_IMAGE_RESOURCE_TEXTURES: usize =
    crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY as usize;

fn image_resource_id_sort_key(id: ImageResourceId) -> (u32, u64) {
    match id {
        ImageResourceId::Renderer(key) => (0, key.0),
        ImageResourceId::Scene(key) => (1, key.0),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "image_resource/tests/identity.rs"]
mod identity_tests;

#[cfg(test)]
mod vector_tests;

#[cfg(test)]
#[path = "image_resource/tests/placement.rs"]
mod placement_tests;
