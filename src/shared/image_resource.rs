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

    pub(crate) fn decode(scope: u32, low: u32, high: u32) -> Self {
        let key = ImageKey(low as u64 | ((high as u64) << 32));
        match scope {
            1 => Self::Scene(key),
            _ => Self::Renderer(key),
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

    pub(crate) fn upload_merged(
        &self,
        scene: &ImageResourceStore,
        max_atlas_dimension: u32,
    ) -> GpuImageResourceUpload {
        GpuImageResourceUpload::from_stores(self, Some(scene), max_atlas_dimension)
    }

    pub(crate) fn upload_signature(
        &self,
        scene: &ImageResourceStore,
        max_atlas_dimension: u32,
    ) -> ImageResourceUploadSignature {
        ImageResourceUploadSignature {
            renderer: self.store_signature(),
            scene: scene.store_signature(),
            max_atlas_dimension,
        }
    }

    pub(crate) fn extend_from(&mut self, other: &ImageResourceStore) {
        self.images.extend(
            other
                .images
                .iter()
                .map(|(&key, image)| (key, image.clone())),
        );
    }

    fn iter(&self) -> impl Iterator<Item = (ImageKey, &Arc<Image>)> {
        self.images.iter().map(|(&key, image)| (key, image))
    }

    fn store_signature(&self) -> ImageResourceStoreSignature {
        let mut entries = self
            .images
            .iter()
            .map(|(&key, image)| {
                (
                    key,
                    image.width,
                    image.height,
                    image.pixels.len() as u64,
                    Arc::as_ptr(image) as usize as u64,
                    image.pixels.as_ptr() as usize as u64,
                )
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.0.0);

        let mut hash = FNV_OFFSET;
        hash = fnv_mix(hash, entries.len() as u64);
        for (key, width, height, pixel_len, image_ptr, pixels_ptr) in entries {
            hash = fnv_mix(hash, key.0);
            hash = fnv_mix(hash, width as u64);
            hash = fnv_mix(hash, height as u64);
            hash = fnv_mix(hash, pixel_len);
            hash = fnv_mix(hash, image_ptr);
            hash = fnv_mix(hash, pixels_ptr);
        }
        ImageResourceStoreSignature {
            len: self.images.len() as u64,
            hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ImageResourceUploadSignature {
    renderer: ImageResourceStoreSignature,
    scene: ImageResourceStoreSignature,
    max_atlas_dimension: u32,
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

#[derive(Clone, Copy, Default)]
pub(crate) struct ImageResourceResolver<'a> {
    renderer: Option<&'a ImageResourceStore>,
    scene: Option<&'a ImageResourceStore>,
}

impl<'a> ImageResourceResolver<'a> {
    pub(crate) fn new(
        renderer: Option<&'a ImageResourceStore>,
        scene: Option<&'a ImageResourceStore>,
    ) -> Self {
        Self { renderer, scene }
    }

    pub(crate) fn resolve(self, id: ImageResourceId) -> Option<&'a Image> {
        match id {
            ImageResourceId::Renderer(key) => {
                self.renderer.and_then(|resources| resources.get(key))
            }
            ImageResourceId::Scene(key) => self.scene.and_then(|resources| resources.get(key)),
        }
    }
}

impl<'a> From<Option<&'a ImageResourceStore>> for ImageResourceResolver<'a> {
    fn from(renderer: Option<&'a ImageResourceStore>) -> Self {
        Self::new(renderer, None)
    }
}

struct ImageResourceEntry<'a> {
    id: ImageResourceId,
    image: &'a Arc<Image>,
}

impl GpuImageResourceUpload {
    fn from_stores(
        renderer: &ImageResourceStore,
        scene: Option<&ImageResourceStore>,
        max_atlas_dimension: u32,
    ) -> Self {
        let mut entries =
            Vec::with_capacity(renderer.images.len() + scene.map_or(0, |scene| scene.images.len()));
        entries.extend(renderer.iter().map(|(key, image)| ImageResourceEntry {
            id: ImageResourceId::renderer(key),
            image,
        }));
        if let Some(scene) = scene {
            entries.extend(scene.iter().map(|(key, image)| ImageResourceEntry {
                id: ImageResourceId::scene(key),
                image,
            }));
        }
        let atlas = ImageResourceAtlas::new(&entries, max_atlas_dimension);
        GpuImageResourceUpload {
            atlas_width: atlas.width,
            atlas_height: atlas.height,
            atlas_pixels: atlas.pixels,
            rects: atlas.rects,
        }
    }

    #[inline]
    pub(crate) fn image_rect(&self, id: ImageResourceId) -> Option<AtlasRect> {
        self.rects.get(&id).copied()
    }
}

#[derive(Clone, Default)]
pub(crate) struct GpuImageResourceUpload {
    pub(crate) atlas_width: u32,
    pub(crate) atlas_height: u32,
    pub(crate) atlas_pixels: Vec<u32>,
    rects: FxHashMap<ImageResourceId, AtlasRect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AtlasRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

struct ImageResourceAtlas {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    rects: FxHashMap<ImageResourceId, AtlasRect>,
}

impl ImageResourceAtlas {
    fn new(entries: &[ImageResourceEntry<'_>], max_dimension: u32) -> Self {
        let Some(pack) = pack_images(entries, max_dimension) else {
            return Self::empty();
        };
        let mut pixels = vec![0; (pack.width * pack.height) as usize];
        for item in &pack.items {
            let image = entries[item.entry_index].image;
            copy_image_with_pad_border(&mut pixels, pack.width, item.x, item.y, image);
        }
        let rects = pack
            .items
            .into_iter()
            .map(|item| {
                (
                    item.id,
                    AtlasRect {
                        x: item.x + 1,
                        y: item.y + 1,
                        width: item.width - 2,
                        height: item.height - 2,
                    },
                )
            })
            .collect();
        Self {
            width: pack.width,
            height: pack.height,
            pixels,
            rects,
        }
    }

    fn empty() -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: vec![0],
            rects: FxHashMap::default(),
        }
    }
}

struct PackItem {
    id: ImageResourceId,
    entry_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct ImagePack {
    width: u32,
    height: u32,
    items: Vec<PackItem>,
}

fn pack_images(entries: &[ImageResourceEntry<'_>], max_dimension: u32) -> Option<ImagePack> {
    if entries.is_empty() || max_dimension < 3 {
        return Some(ImagePack {
            width: 1,
            height: 1,
            items: Vec::new(),
        });
    }

    let mut sizes = entries
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            (
                entry.id,
                entry_index,
                entry.image.width + 2,
                entry.image.height + 2,
            )
        })
        .collect::<Vec<_>>();
    sizes.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| {
                image_resource_id_sort_key(left.0).cmp(&image_resource_id_sort_key(right.0))
            })
    });
    if sizes
        .iter()
        .any(|(_, _, width, height)| *width > max_dimension || *height > max_dimension)
    {
        return None;
    }

    let total_area = sizes
        .iter()
        .map(|(_, _, width, height)| width.saturating_mul(*height) as u64)
        .sum::<u64>();
    let max_width = sizes
        .iter()
        .map(|(_, _, width, _)| *width)
        .max()
        .unwrap_or(1);
    let mut width = max_width
        .max((total_area as f64).sqrt().ceil() as u32)
        .next_power_of_two()
        .min(max_dimension);

    loop {
        if let Some(pack) = try_pack_images(&sizes, width, max_dimension) {
            return Some(pack);
        }
        if width == max_dimension {
            return None;
        }
        width = width.saturating_mul(2).min(max_dimension);
    }
}

fn try_pack_images(
    sizes: &[(ImageResourceId, usize, u32, u32)],
    atlas_width: u32,
    max_dimension: u32,
) -> Option<ImagePack> {
    let mut items = Vec::with_capacity(sizes.len());
    let mut x = 0;
    let mut y = 0;
    let mut row_height = 0;
    for &(id, entry_index, width, height) in sizes {
        if x + width > atlas_width {
            y += row_height;
            x = 0;
            row_height = 0;
        }
        if y + height > max_dimension {
            return None;
        }
        items.push(PackItem {
            id,
            entry_index,
            x,
            y,
            width,
            height,
        });
        x += width;
        row_height = row_height.max(height);
    }
    let height = (y + row_height)
        .max(1)
        .next_power_of_two()
        .min(max_dimension);
    Some(ImagePack {
        width: atlas_width.max(1),
        height,
        items,
    })
}

fn image_resource_id_sort_key(id: ImageResourceId) -> (u32, u64) {
    match id {
        ImageResourceId::Renderer(key) => (0, key.0),
        ImageResourceId::Scene(key) => (1, key.0),
    }
}

fn copy_image_with_pad_border(
    atlas: &mut [u32],
    atlas_width: u32,
    atlas_x: u32,
    atlas_y: u32,
    image: &Image,
) {
    // Duplicate edge texels into a 1px guard band so hardware bilinear sampling
    // can clamp pad images without bleeding into neighboring atlas entries.
    for y in 0..image.height + 2 {
        let src_y = y.saturating_sub(1).min(image.height - 1);
        for x in 0..image.width + 2 {
            let src_x = x.saturating_sub(1).min(image.width - 1);
            let src = image.pixels[(src_y * image.width + src_x) as usize];
            atlas[((atlas_y + y) * atlas_width + atlas_x + x) as usize] = src;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageKey, ImageResourceId, ImageResourceStore};
    use crate::shared::image::Image;

    #[test]
    fn remove_and_clear_update_image_resource_uploads() {
        let mut resources = ImageResourceStore::default();
        let first = ImageKey::new(1);
        let second = ImageKey::new(2);

        assert!(resources.insert(first, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
        assert!(resources.insert(second, Image::from_rgba8(1, 1, [0, 255, 0, 255])));
        let scene = ImageResourceStore::default();
        let upload = resources.upload_merged(&scene, 4096);
        assert!(
            upload
                .image_rect(ImageResourceId::renderer(first))
                .is_some()
        );
        assert!(
            upload
                .image_rect(ImageResourceId::renderer(second))
                .is_some()
        );

        assert!(resources.remove(first));
        assert!(!resources.remove(first));
        let upload = resources.upload_merged(&scene, 4096);
        assert!(
            upload
                .image_rect(ImageResourceId::renderer(first))
                .is_none()
        );
        assert!(
            upload
                .image_rect(ImageResourceId::renderer(second))
                .is_some()
        );

        assert!(resources.clear());
        assert!(!resources.clear());
        let upload = resources.upload_merged(&scene, 4096);
        assert_eq!(upload.atlas_width, 1);
        assert_eq!(upload.atlas_height, 1);
        assert_eq!(upload.atlas_pixels.len(), 1);
        assert!(
            upload
                .image_rect(ImageResourceId::renderer(second))
                .is_none()
        );
    }
}
