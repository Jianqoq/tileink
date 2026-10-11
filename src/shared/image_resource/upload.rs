//! Resolve one complete renderer/scene snapshot into immutable GPU placements.
use super::atlas::{
    ImageResourcePageSignature, atlas_page_size, can_use_atlas, copy_image_with_pad_border,
    pack_images_into_pages, should_use_texture_table,
};
use super::source::ImageIdentityGuard;
use super::{
    ImageKey, ImageResourceId, ImageResourceStore, ImageSource, MAX_IMAGE_RESOURCE_TEXTURES,
    fnv_mix, image_entry_hash, image_resource_id_sort_key,
};
use rustc_hash::FxHashMap;
use std::rc::Rc;

pub(super) struct ImageResourceEntry<'a> {
    pub(super) id: ImageResourceId,
    pub(super) image: &'a ImageSource,
    pub(super) signature: ImageEntrySignature,
}

impl GpuImageResourceUpload {
    pub(super) fn from_stores(
        renderer: &ImageResourceStore,
        scene: &ImageResourceStore,
        max_atlas_dimension: u32,
        max_atlas_pages: u32,
        texture_table_len: u32,
        previous: Option<&GpuImageResourceUpload>,
    ) -> Self {
        let mut entries = Vec::with_capacity(renderer.images.len() + scene.images.len());
        entries.extend(renderer.iter().map(|(key, image)| ImageResourceEntry {
            id: ImageResourceId::renderer(key),
            image,
            signature: ImageEntrySignature::new(ImageResourceId::renderer(key), image),
        }));
        entries.extend(scene.iter().map(|(key, image)| ImageResourceEntry {
            id: ImageResourceId::scene(key),
            image,
            signature: ImageEntrySignature::new(ImageResourceId::scene(key), image),
        }));
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

    #[cfg(test)]
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

    pub(crate) fn vectors(&self) -> &[GpuVectorImageUpload] {
        &self.vectors
    }

    pub(crate) fn textures(&self) -> &[GpuImageResourceTextureUpload] {
        &self.textures
    }
}

#[derive(Clone)]
pub(crate) struct GpuImageResourceUpload {
    // Signatures use Rc addresses, so their allocation identities must outlive the cache.
    // Weak guards prevent address reuse and in-place mutation after a scene is dropped,
    // without retaining its pixel buffers. This fixes stale-image reuse at its source.
    _image_identities: Vec<ImageIdentityGuard>,
    generation: u64,
    atlas_page_size: u32,
    atlas_pages: Vec<GpuImageResourceAtlasPageUpload>,
    textures: Vec<GpuImageResourceTextureUpload>,
    vectors: Vec<GpuVectorImageUpload>,
    placements: FxHashMap<ImageResourceId, ImageResourcePlacement>,
}

/// Work the selected GPU executor must resolve before sampling the placement.
/// Atlas destinations include a duplicated one-pixel border outside the inner rect.
#[derive(Clone)]
pub(crate) struct GpuVectorImageUpload {
    pub(crate) canvas: Rc<crate::Canvas>,
    pub(crate) placement: ImageResourcePlacement,
    #[cfg(test)]
    pub(crate) dirty: bool,
}

// Native submissions retain upload pixels until GPU completion.
pub(crate) type ImagePixels = Rc<Vec<u32>>;

fn image_pixels(pixels: Vec<u32>) -> ImagePixels {
    Rc::new(pixels)
}

#[derive(Clone)]
pub(crate) struct GpuImageResourceAtlasPageUpload {
    pub(crate) index: u32,
    pub(crate) size: u32,
    pub(crate) pixels: ImagePixels,
    #[cfg(test)]
    pub(crate) dirty: bool,
    signature: ImageResourcePageSignature,
}

#[derive(Clone)]
pub(crate) struct GpuImageResourceTextureUpload {
    pub(crate) index: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: ImagePixels,
    #[cfg(test)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ImageEntrySignature {
    pub(super) hash: u64,
}

impl ImageEntrySignature {
    fn new(id: ImageResourceId, image: &ImageSource) -> Self {
        let (namespace, key) = image_resource_id_sort_key(id);
        Self {
            hash: fnv_mix(
                image_entry_hash(ImageKey::new(key), image),
                namespace as u64,
            ),
        }
    }
}

struct ImageResourceUploadBuilder<'a> {
    entries: &'a [ImageResourceEntry<'a>],
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
                pixels: image_pixels(vec![0]),
                #[cfg(test)]
                dirty: previous.is_none_or(|prev| prev.atlas_pages.is_empty()),
                signature: ImageResourcePageSignature::default(),
            });
        } else {
            for (page_index, page) in atlas_pages.into_iter().enumerate() {
                let previous_page = previous.and_then(|prev| prev.atlas_pages.get(page_index));
                let dirty = previous_page.is_none_or(|prev| {
                    prev.size != self.page_size || prev.signature != page.signature
                });
                let pixels = if let Some(previous) = previous_page.filter(|_| !dirty) {
                    previous.pixels.clone()
                } else {
                    let mut pixels = vec![0; (self.page_size * self.page_size) as usize];
                    for item in &page.items {
                        if let Some(image) = self.entries[item.entry_index].image.raster() {
                            copy_image_with_pad_border(
                                &mut pixels,
                                self.page_size,
                                item.x,
                                item.y,
                                image,
                            );
                        }
                    }
                    image_pixels(pixels)
                };
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
                    #[cfg(test)]
                    dirty,
                    signature: page.signature,
                });
            }
        }

        let mut textures = Vec::with_capacity(self.texture_entry_indices.len());
        for (texture_index, &entry_index) in self.texture_entry_indices.iter().enumerate() {
            let entry = &self.entries[entry_index];
            let previous_texture = previous.and_then(|prev| prev.textures.get(texture_index));
            let dirty = previous_texture.is_none_or(|prev| {
                prev.width != entry.image.width()
                    || prev.height != entry.image.height()
                    || prev.signature != entry.signature
            });
            let pixels = if let Some(previous) = previous_texture.filter(|_| !dirty) {
                previous.pixels.clone()
            } else {
                image_pixels(
                    entry
                        .image
                        .raster()
                        .map(|image| image.pixels.clone())
                        .unwrap_or_default(),
                )
            };
            placements.insert(
                entry.id,
                ImageResourcePlacement::Texture(TextureRect {
                    index: texture_index as u32,
                    width: entry.image.width(),
                    height: entry.image.height(),
                }),
            );
            textures.push(GpuImageResourceTextureUpload {
                index: texture_index as u32,
                width: entry.image.width(),
                height: entry.image.height(),
                pixels,
                #[cfg(test)]
                dirty,
                signature: entry.signature,
            });
        }

        let vectors = self
            .entries
            .iter()
            .filter_map(|entry| {
                let ImageSource::Vector(canvas) = entry.image else {
                    return None;
                };
                let placement = *placements.get(&entry.id)?;
                #[cfg(test)]
                let dirty = match placement {
                    ImageResourcePlacement::Atlas(rect) => page_uploads[rect.page as usize].dirty,
                    ImageResourcePlacement::Texture(rect) => textures[rect.index as usize].dirty,
                };
                Some(GpuVectorImageUpload {
                    canvas: Rc::clone(canvas),
                    placement,
                    #[cfg(test)]
                    dirty,
                })
            })
            .collect();

        GpuImageResourceUpload {
            _image_identities: self
                .entries
                .iter()
                .map(|entry| entry.image.guard())
                .collect(),
            generation: previous.map_or(1, |upload| upload.generation.wrapping_add(1)),
            // Every upload owns either packed pages or its initialized empty atlas.
            atlas_page_size: page_uploads[0].size,
            atlas_pages: page_uploads,
            textures,
            vectors,
            placements,
        }
    }
}
