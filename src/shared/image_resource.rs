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
    signature_hash: u64,
}

impl ImageResourceStore {
    pub(crate) fn insert(&mut self, key: ImageKey, image: impl Into<Arc<Image>>) -> bool {
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return false;
        }
        if let Some(previous) = self.images.insert(key, image.clone()) {
            self.signature_hash ^= image_entry_hash(key, &previous);
        }
        self.signature_hash ^= image_entry_hash(key, &image);
        true
    }

    pub(crate) fn get(&self, key: ImageKey) -> Option<&Image> {
        self.images.get(&key).map(Arc::as_ref)
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
            Some(scene),
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

    pub(crate) fn iter(&self) -> impl Iterator<Item = (ImageKey, &Arc<Image>)> {
        self.images.iter().map(|(&key, image)| (key, image))
    }

    fn store_signature(&self) -> ImageResourceStoreSignature {
        ImageResourceStoreSignature {
            len: self.images.len() as u64,
            hash: self.signature_hash,
        }
    }
}

fn image_entry_hash(key: ImageKey, image: &Arc<Image>) -> u64 {
    let mut hash = FNV_OFFSET;
    hash = fnv_mix(hash, key.0);
    hash = fnv_mix(hash, image.width as u64);
    hash = fnv_mix(hash, image.height as u64);
    hash = fnv_mix(hash, image.pixels.len() as u64);
    hash = fnv_mix(hash, Arc::as_ptr(image) as usize as u64);
    fnv_mix(hash, image.pixels.as_ptr() as usize as u64)
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
struct ImageResourceEntry<'a> {
    id: ImageResourceId,
    image: &'a Arc<Image>,
    signature: ImageEntrySignature,
}

impl GpuImageResourceUpload {
    fn from_stores(
        renderer: &ImageResourceStore,
        scene: Option<&ImageResourceStore>,
        max_atlas_dimension: u32,
        max_atlas_pages: u32,
        texture_table_len: u32,
        previous: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut entries =
            Vec::with_capacity(renderer.images.len() + scene.map_or(0, |scene| scene.images.len()));
        entries.extend(renderer.iter().map(|(key, image)| ImageResourceEntry {
            id: ImageResourceId::renderer(key),
            image,
            signature: ImageEntrySignature::new(ImageResourceId::renderer(key), image),
        }));
        if let Some(scene) = scene {
            entries.extend(scene.iter().map(|(key, image)| ImageResourceEntry {
                id: ImageResourceId::scene(key),
                image,
                signature: ImageEntrySignature::new(ImageResourceId::scene(key), image),
            }));
        }
        entries.sort_by_key(|entry| image_resource_id_sort_key(entry.id));
        ImageResourceUploadBuilder::new(
            &entries,
            max_atlas_dimension,
            max_atlas_pages,
            texture_table_len,
        )
        .build(previous)
    }

    #[inline]
    pub(crate) fn image_placement(&self, id: ImageResourceId) -> Option<ImageResourcePlacement> {
        self.placements.get(&id).copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// Changes whenever resource placement data is rebuilt. Scene brush uploads use this to
    /// distinguish a placement-table change, which requires repatching every image brush, from
    /// an ordinary retained mutation, which only requires patching dirty brush allocations.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn atlas_page_size(&self) -> u32 {
        self.atlas_page_size
    }

    pub(crate) fn atlas_page_count(&self) -> u32 {
        self.atlas_pages.len() as u32
    }

    pub(crate) fn atlas_pages(&self) -> &[GpuImageResourceAtlasPageUpload] {
        &self.atlas_pages
    }

    #[cfg(test)]
    pub(crate) fn atlas_pages_mut(&mut self) -> &mut [GpuImageResourceAtlasPageUpload] {
        &mut self.atlas_pages
    }

    pub(crate) fn textures(&self) -> &[GpuImageResourceTextureUpload] {
        &self.textures
    }
}

#[derive(Clone, Default)]
pub(crate) struct GpuImageResourceUpload {
    generation: u64,
    atlas_page_size: u32,
    atlas_pages: Vec<GpuImageResourceAtlasPageUpload>,
    textures: Vec<GpuImageResourceTextureUpload>,
    placements: FxHashMap<ImageResourceId, ImageResourcePlacement>,
}

#[derive(Clone)]
pub(crate) struct GpuImageResourceAtlasPageUpload {
    pub(crate) index: u32,
    pub(crate) size: u32,
    pub(crate) pixels: Vec<u32>,
    pub(crate) dirty: bool,
    signature: ImageResourcePageSignature,
}

#[derive(Clone)]
pub(crate) struct GpuImageResourceTextureUpload {
    pub(crate) index: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u32>,
    pub(crate) dirty: bool,
    signature: ImageEntrySignature,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AtlasRect {
    pub(crate) page: u32,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextureRect {
    pub(crate) index: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageResourcePlacement {
    Atlas(AtlasRect),
    Texture(TextureRect),
}

struct PackItem {
    id: ImageResourceId,
    entry_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct ImagePackPage {
    items: Vec<PackItem>,
    signature: ImageResourcePageSignature,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ImageEntrySignature {
    hash: u64,
}

impl ImageEntrySignature {
    fn new(id: ImageResourceId, image: &Arc<Image>) -> Self {
        let mut hash = FNV_OFFSET;
        hash = fnv_mix(hash, image_resource_id_sort_key(id).0 as u64);
        hash = fnv_mix(hash, image_resource_id_sort_key(id).1);
        hash = fnv_mix(hash, image.width as u64);
        hash = fnv_mix(hash, image.height as u64);
        hash = fnv_mix(hash, image.pixels.len() as u64);
        hash = fnv_mix(hash, Arc::as_ptr(image) as usize as u64);
        hash = fnv_mix(hash, image.pixels.as_ptr() as usize as u64);
        Self { hash }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ImageResourcePageSignature {
    hash: u64,
}

impl ImageResourcePageSignature {
    fn new(items: &[PackItem], entries: &[ImageResourceEntry<'_>], page_size: u32) -> Self {
        let mut hash = FNV_OFFSET;
        hash = fnv_mix(hash, page_size as u64);
        hash = fnv_mix(hash, items.len() as u64);
        for item in items {
            let entry = &entries[item.entry_index];
            hash = fnv_mix(hash, entry.signature.hash);
            hash = fnv_mix(hash, item.x as u64);
            hash = fnv_mix(hash, item.y as u64);
            hash = fnv_mix(hash, item.width as u64);
            hash = fnv_mix(hash, item.height as u64);
        }
        Self { hash }
    }
}

struct ImageResourceUploadBuilder<'a> {
    entries: &'a [ImageResourceEntry<'a>],
    max_atlas_dimension: u32,
    max_atlas_pages: u32,
    page_size: u32,
    atlas_entry_indices: Vec<usize>,
    texture_entry_indices: Vec<usize>,
}

impl<'a> ImageResourceUploadBuilder<'a> {
    fn new(
        entries: &'a [ImageResourceEntry<'a>],
        max_atlas_dimension: u32,
        max_atlas_pages: u32,
        texture_table_len: u32,
    ) -> Self {
        let max_atlas_dimension = max_atlas_dimension.max(1);
        let texture_table_len = texture_table_len.min(MAX_IMAGE_RESOURCE_TEXTURES as u32) as usize;
        let mut texture_entry_indices = Vec::new();
        let mut atlas_entry_indices = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if texture_entry_indices.len() < texture_table_len
                && should_use_texture_table(entry.image, max_atlas_dimension)
            {
                texture_entry_indices.push(index);
            } else if can_use_atlas(entry.image, max_atlas_dimension) {
                atlas_entry_indices.push(index);
            }
        }
        let page_size = atlas_page_size(entries, &atlas_entry_indices, max_atlas_dimension);
        Self {
            entries,
            max_atlas_dimension,
            max_atlas_pages,
            page_size,
            atlas_entry_indices,
            texture_entry_indices,
        }
    }

    fn build(self, previous: Option<&GpuImageResourceUpload>) -> GpuImageResourceUpload {
        let atlas_pages = pack_images_into_pages(
            self.entries,
            &self.atlas_entry_indices,
            self.page_size,
            self.max_atlas_pages,
        );
        let mut placements = FxHashMap::default();
        let mut page_uploads = Vec::with_capacity(atlas_pages.len().max(1));

        if atlas_pages.is_empty() {
            page_uploads.push(GpuImageResourceAtlasPageUpload {
                index: 0,
                size: 1,
                pixels: vec![0],
                dirty: previous.is_none_or(|prev| prev.atlas_pages.is_empty()),
                signature: ImageResourcePageSignature::default(),
            });
        } else {
            for (page_index, page) in atlas_pages.into_iter().enumerate() {
                let previous_page = previous.and_then(|prev| prev.atlas_pages.get(page_index));
                let dirty = previous_page.is_none_or(|prev| {
                    prev.size != self.page_size || prev.signature != page.signature
                });
                let mut pixels = if dirty {
                    vec![0; (self.page_size * self.page_size) as usize]
                } else {
                    previous_page
                        .map(|prev| prev.pixels.clone())
                        .unwrap_or_else(|| vec![0; (self.page_size * self.page_size) as usize])
                };
                if dirty {
                    for item in &page.items {
                        let image = self.entries[item.entry_index].image;
                        copy_image_with_pad_border(
                            &mut pixels,
                            self.page_size,
                            item.x,
                            item.y,
                            image,
                        );
                    }
                }
                for item in page.items {
                    placements.insert(
                        item.id,
                        ImageResourcePlacement::Atlas(AtlasRect {
                            page: page_index as u32,
                            x: item.x + 1,
                            y: item.y + 1,
                            width: item.width - 2,
                            height: item.height - 2,
                        }),
                    );
                }
                page_uploads.push(GpuImageResourceAtlasPageUpload {
                    index: page_index as u32,
                    size: self.page_size,
                    pixels,
                    dirty,
                    signature: page.signature,
                });
            }
        }

        let mut textures = Vec::with_capacity(self.texture_entry_indices.len());
        for (texture_index, &entry_index) in self.texture_entry_indices.iter().enumerate() {
            let entry = &self.entries[entry_index];
            if entry.image.width > self.max_atlas_dimension
                || entry.image.height > self.max_atlas_dimension
            {
                continue;
            }
            let previous_texture = previous.and_then(|prev| prev.textures.get(texture_index));
            let dirty = previous_texture.is_none_or(|prev| {
                prev.width != entry.image.width
                    || prev.height != entry.image.height
                    || prev.signature != entry.signature
            });
            let pixels = if dirty {
                entry.image.pixels.clone()
            } else {
                previous_texture
                    .map(|prev| prev.pixels.clone())
                    .unwrap_or_else(|| entry.image.pixels.clone())
            };
            placements.insert(
                entry.id,
                ImageResourcePlacement::Texture(TextureRect {
                    index: texture_index as u32,
                    width: entry.image.width,
                    height: entry.image.height,
                }),
            );
            textures.push(GpuImageResourceTextureUpload {
                index: texture_index as u32,
                width: entry.image.width,
                height: entry.image.height,
                pixels,
                dirty,
                signature: entry.signature,
            });
        }

        GpuImageResourceUpload {
            generation: previous.map_or(1, |upload| upload.generation.wrapping_add(1)),
            atlas_page_size: page_uploads.first().map_or(1, |page| page.size),
            atlas_pages: page_uploads,
            textures,
            placements,
        }
    }
}

const DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE: u32 = 2048;
const LARGE_IMAGE_AREA_THRESHOLD: u64 = 2048 * 2048;
pub(crate) const MAX_IMAGE_RESOURCE_TEXTURES: usize = 64;

fn should_use_texture_table(image: &Image, max_atlas_dimension: u32) -> bool {
    if image.width > max_atlas_dimension || image.height > max_atlas_dimension {
        return false;
    }
    let padded_width = image.width.saturating_add(2);
    let padded_height = image.height.saturating_add(2);
    padded_width > DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        || padded_height > DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        || (image.width as u64 * image.height as u64) > LARGE_IMAGE_AREA_THRESHOLD
}

fn can_use_atlas(image: &Image, max_atlas_dimension: u32) -> bool {
    image.width.saturating_add(2) <= max_atlas_dimension
        && image.height.saturating_add(2) <= max_atlas_dimension
}

fn atlas_page_size(
    entries: &[ImageResourceEntry<'_>],
    atlas_entry_indices: &[usize],
    max_dimension: u32,
) -> u32 {
    let required = atlas_entry_indices
        .iter()
        .map(|&index| {
            let image = entries[index].image;
            image
                .width
                .saturating_add(2)
                .max(image.height.saturating_add(2))
        })
        .max()
        .unwrap_or(1);
    DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        .max(required)
        .next_power_of_two()
        .min(max_dimension.max(1))
        .max(1)
}

fn pack_images_into_pages(
    entries: &[ImageResourceEntry<'_>],
    atlas_entry_indices: &[usize],
    page_size: u32,
    max_pages: u32,
) -> Vec<ImagePackPage> {
    if atlas_entry_indices.is_empty() {
        return Vec::new();
    }
    let max_pages = max_pages as usize;
    if page_size < 3 || max_pages == 0 {
        return Vec::new();
    }

    let mut sizes = atlas_entry_indices
        .iter()
        .map(|&entry_index| {
            let entry = &entries[entry_index];
            (
                entry.id,
                entry_index,
                entry.image.width.saturating_add(2),
                entry.image.height.saturating_add(2),
            )
        })
        .filter(|(_, _, width, height)| *width <= page_size && *height <= page_size)
        .collect::<Vec<_>>();
    if sizes.is_empty() {
        return Vec::new();
    }
    sizes.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| {
                image_resource_id_sort_key(left.0).cmp(&image_resource_id_sort_key(right.0))
            })
    });

    let mut pages = Vec::new();
    let mut current = PagePacker::new(page_size);
    for item in sizes {
        if !current.push(item) {
            if !current.is_empty() {
                if pages.len() >= max_pages {
                    return pages;
                }
                pages.push(current.finish(entries));
                if pages.len() >= max_pages {
                    return pages;
                }
            }
            current = PagePacker::new(page_size);
            if !current.push(item) {
                continue;
            }
        }
    }
    if !current.is_empty() && pages.len() < max_pages {
        pages.push(current.finish(entries));
    }
    pages
}

struct PagePacker {
    size: u32,
    x: u32,
    y: u32,
    row_height: u32,
    items: Vec<PackItem>,
}

impl PagePacker {
    fn new(size: u32) -> Self {
        Self {
            size,
            x: 0,
            y: 0,
            row_height: 0,
            items: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn push(&mut self, item: (ImageResourceId, usize, u32, u32)) -> bool {
        let (id, entry_index, width, height) = item;
        let mut x = self.x;
        let mut y = self.y;
        let mut row_height = self.row_height;
        if x + width > self.size {
            y += row_height;
            x = 0;
            row_height = 0;
        }
        if y + height > self.size {
            return false;
        }
        self.items.push(PackItem {
            id,
            entry_index,
            x,
            y,
            width,
            height,
        });
        self.x = x + width;
        self.y = y;
        self.row_height = row_height.max(height);
        true
    }

    fn finish(self, entries: &[ImageResourceEntry<'_>]) -> ImagePackPage {
        let signature = ImageResourcePageSignature::new(&self.items, entries, self.size);
        ImagePackPage {
            items: self.items,
            signature,
        }
    }
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
    use super::{
        ImageKey, ImageResourceId, ImageResourcePlacement, ImageResourceStore,
        MAX_IMAGE_RESOURCE_TEXTURES,
    };
    use crate::shared::image::Image;

    #[test]
    fn remove_and_clear_update_image_resource_uploads() {
        let mut resources = ImageResourceStore::default();
        let first = ImageKey::new(1);
        let second = ImageKey::new(2);

        assert!(resources.insert(first, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
        assert!(resources.insert(second, Image::from_rgba8(1, 1, [0, 255, 0, 255])));
        let scene = ImageResourceStore::default();
        let upload = resources.upload_merged(&scene, 4096, 256, 0, None);
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(first))
                .is_some()
        );
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(second))
                .is_some()
        );

        assert!(resources.remove(first));
        assert!(!resources.remove(first));
        let upload = resources.upload_merged(&scene, 4096, 256, 0, Some(&upload));
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(first))
                .is_none()
        );
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(second))
                .is_some()
        );

        assert!(resources.clear());
        assert!(!resources.clear());
        let upload = resources.upload_merged(&scene, 4096, 256, 0, Some(&upload));
        assert_eq!(upload.atlas_page_size(), 1);
        assert_eq!(upload.atlas_page_count(), 1);
        assert_eq!(upload.atlas_pages()[0].pixels.len(), 1);
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(second))
                .is_none()
        );
    }

    #[test]
    fn small_images_pack_into_multiple_atlas_pages() {
        let mut resources = ImageResourceStore::default();
        for key in 1..=4 {
            assert!(resources.insert(
                ImageKey::new(key),
                solid_image(4, 4, [key as u8, 0, 0, 255])
            ));
        }
        let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, None);

        assert_eq!(upload.atlas_page_size(), 8);
        assert_eq!(upload.atlas_page_count(), 4);
        for key in 1..=4 {
            assert!(matches!(
                upload.image_placement(ImageResourceId::renderer(ImageKey::new(key))),
                Some(ImageResourcePlacement::Atlas(_))
            ));
        }
    }

    #[test]
    fn atlas_page_count_is_capped_by_device_layer_limit() {
        let mut resources = ImageResourceStore::default();
        for key in 1..=4 {
            assert!(resources.insert(
                ImageKey::new(key),
                solid_image(4, 4, [key as u8, 0, 0, 255])
            ));
        }

        let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 2, 0, None);

        assert_eq!(upload.atlas_page_count(), 2);
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(ImageKey::new(1))),
            Some(ImageResourcePlacement::Atlas(rect)) if rect.page < 2
        ));
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(ImageKey::new(2))),
            Some(ImageResourcePlacement::Atlas(rect)) if rect.page < 2
        ));
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(ImageKey::new(3)))
                .is_none()
        );
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(ImageKey::new(4)))
                .is_none()
        );
    }

    #[test]
    fn oversized_atlas_image_does_not_drop_other_atlas_images() {
        let mut resources = ImageResourceStore::default();
        let small = ImageKey::new(1);
        let oversized = ImageKey::new(2);
        assert!(resources.insert(small, solid_image(1, 1, [255, 0, 0, 255])));
        assert!(resources.insert(oversized, solid_image(9, 1, [0, 255, 0, 255])));

        let upload = resources.upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);

        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(small)),
            Some(ImageResourcePlacement::Atlas(_))
        ));
        assert!(
            upload
                .image_placement(ImageResourceId::renderer(oversized))
                .is_none()
        );
    }

    #[test]
    fn large_images_use_texture_table_when_enabled() {
        let key = ImageKey::new(7);
        let mut resources = ImageResourceStore::default();
        assert!(resources.insert(key, solid_image(2050, 1, [255, 0, 0, 255])));

        let upload = resources.upload_merged(
            &ImageResourceStore::default(),
            4096,
            256,
            MAX_IMAGE_RESOURCE_TEXTURES as u32,
            None,
        );

        assert_eq!(upload.textures().len(), 1);
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(key)),
            Some(ImageResourcePlacement::Texture(texture)) if texture.index == 0 && texture.width == 2050
        ));
    }

    #[test]
    fn large_image_texture_table_respects_actual_table_len() {
        let mut resources = ImageResourceStore::default();
        assert!(resources.insert(ImageKey::new(1), solid_image(2050, 1, [255, 0, 0, 255])));
        assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 255, 0, 255])));

        let upload = resources.upload_merged(&ImageResourceStore::default(), 4096, 4, 1, None);

        assert_eq!(upload.textures().len(), 1);
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(ImageKey::new(1))),
            Some(ImageResourcePlacement::Texture(texture)) if texture.index == 0
        ));
        assert!(matches!(
            upload.image_placement(ImageResourceId::renderer(ImageKey::new(2))),
            Some(ImageResourcePlacement::Atlas(_))
        ));
    }

    #[test]
    fn unchanged_upload_marks_pages_and_textures_clean() {
        let mut resources = ImageResourceStore::default();
        assert!(resources.insert(ImageKey::new(1), solid_image(4, 4, [255, 0, 0, 255])));
        assert!(resources.insert(ImageKey::new(2), solid_image(2050, 1, [0, 255, 0, 255])));
        let first = resources.upload_merged(
            &ImageResourceStore::default(),
            4096,
            256,
            MAX_IMAGE_RESOURCE_TEXTURES as u32,
            None,
        );

        let second = resources.upload_merged(
            &ImageResourceStore::default(),
            4096,
            256,
            MAX_IMAGE_RESOURCE_TEXTURES as u32,
            Some(&first),
        );

        assert!(second.atlas_pages().iter().all(|page| !page.dirty));
        assert!(second.textures().iter().all(|texture| !texture.dirty));
    }

    #[test]
    fn updating_one_atlas_page_dirties_only_that_page() {
        let mut resources = ImageResourceStore::default();
        assert!(resources.insert(ImageKey::new(1), solid_image(4, 4, [255, 0, 0, 255])));
        assert!(resources.insert(ImageKey::new(2), solid_image(4, 4, [0, 255, 0, 255])));
        let first = resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, None);

        assert!(resources.insert(ImageKey::new(2), solid_image(4, 4, [0, 0, 255, 255])));
        let second =
            resources.upload_merged(&ImageResourceStore::default(), 8, 256, 0, Some(&first));

        let dirty_pages = second
            .atlas_pages()
            .iter()
            .filter(|page| page.dirty)
            .count();
        assert_eq!(dirty_pages, 1);
    }

    #[test]
    fn updating_one_large_texture_dirties_only_that_texture() {
        let mut resources = ImageResourceStore::default();
        assert!(resources.insert(ImageKey::new(1), solid_image(2050, 1, [255, 0, 0, 255])));
        assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 255, 0, 255])));
        let first = resources.upload_merged(
            &ImageResourceStore::default(),
            4096,
            256,
            MAX_IMAGE_RESOURCE_TEXTURES as u32,
            None,
        );

        assert!(resources.insert(ImageKey::new(2), solid_image(2051, 1, [0, 0, 255, 255])));
        let second = resources.upload_merged(
            &ImageResourceStore::default(),
            4096,
            256,
            MAX_IMAGE_RESOURCE_TEXTURES as u32,
            Some(&first),
        );

        assert_eq!(second.textures().len().min(MAX_IMAGE_RESOURCE_TEXTURES), 2);
        let dirty_textures = second
            .textures()
            .iter()
            .filter(|texture| texture.dirty)
            .count();
        assert_eq!(dirty_textures, 1);
    }

    fn solid_image(width: u32, height: u32, color: [u8; 4]) -> Image {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            rgba.extend_from_slice(&color);
        }
        Image::from_rgba8(width, height, rgba)
    }
}
