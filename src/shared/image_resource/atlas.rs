//! Deterministic shelf packing and guarded atlas texel copies.
use super::upload::ImageResourceEntry;
use super::{FNV_OFFSET, Image, ImageResourceId, ImageSource, fnv_mix, image_resource_id_sort_key};

pub(super) struct PackItem {
    pub(super) id: ImageResourceId,
    pub(super) entry_index: usize,
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) struct ImagePackPage {
    pub(super) items: Vec<PackItem>,
    pub(super) signature: ImageResourcePageSignature,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ImageResourcePageSignature {
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

const DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE: u32 = 2048;
const LARGE_IMAGE_AREA_THRESHOLD: u64 = 2048 * 2048;
pub(super) fn should_use_texture_table(image: &ImageSource, max_atlas_dimension: u32) -> bool {
    if image.width() > max_atlas_dimension || image.height() > max_atlas_dimension {
        return false;
    }
    let padded_width = image.width().saturating_add(2);
    let padded_height = image.height().saturating_add(2);
    padded_width > DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        || padded_height > DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        || (image.width() as u64 * image.height() as u64) > LARGE_IMAGE_AREA_THRESHOLD
}

pub(super) fn can_use_atlas(image: &ImageSource, max_atlas_dimension: u32) -> bool {
    image.width().saturating_add(2) <= max_atlas_dimension
        && image.height().saturating_add(2) <= max_atlas_dimension
}

pub(super) fn atlas_page_size(
    entries: &[ImageResourceEntry<'_>],
    atlas_entry_indices: &[usize],
    max_dimension: u32,
) -> u32 {
    let required = atlas_entry_indices
        .iter()
        .map(|&index| {
            let image = entries[index].image;
            image
                .width()
                .saturating_add(2)
                .max(image.height().saturating_add(2))
        })
        .max()
        .unwrap_or(1);
    DEFAULT_IMAGE_RESOURCE_ATLAS_PAGE_SIZE
        .max(required)
        .next_power_of_two()
        .min(max_dimension)
}

pub(super) fn pack_images_into_pages(
    entries: &[ImageResourceEntry<'_>],
    atlas_entry_indices: &[usize],
    page_size: u32,
    max_pages: u32,
) -> Vec<ImagePackPage> {
    if atlas_entry_indices.is_empty() || max_pages == 0 {
        return Vec::new();
    }
    let max_pages = max_pages as usize;

    let mut sizes = atlas_entry_indices
        .iter()
        .map(|&entry_index| {
            let entry = &entries[entry_index];
            (
                entry.id,
                entry_index,
                entry.image.width().saturating_add(2),
                entry.image.height().saturating_add(2),
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

    let mut pages = Vec::new();
    // Selection guarantees every item fits a fresh page, including its border.
    // Seed each page with that item instead of retrying a fallible empty packer.
    let mut current = PagePacker::new(page_size, sizes[0]);
    for item in sizes.into_iter().skip(1) {
        if !current.push(item) {
            pages.push(current.finish(entries));
            if pages.len() == max_pages {
                return pages;
            }
            current = PagePacker::new(page_size, item);
        }
    }
    pages.push(current.finish(entries));
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
    fn new(size: u32, item: (ImageResourceId, usize, u32, u32)) -> Self {
        let (id, entry_index, width, height) = item;
        let mut items = Vec::with_capacity(4);
        items.push(PackItem {
            id,
            entry_index,
            x: 0,
            y: 0,
            width,
            height,
        });
        Self {
            size,
            x: width,
            y: 0,
            row_height: height,
            items,
        }
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

pub(super) fn copy_image_with_pad_border(
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
