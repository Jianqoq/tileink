//! Deferred vector-image identity and exact GPU copy planning.

#[cfg(feature = "wgpu")]
use super::resource_writes::ReadyResource;
use crate::{Canvas, shared::image_resource::ImageResourcePlacement};
use rustc_hash::FxHashMap;
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageCopyRegion {
    pub(crate) source: [u32; 2],
    pub(crate) destination: [u32; 3],
    pub(crate) extent: [u32; 2],
}

/// Reproduce the raster uploader's one-pixel clamp border using texture copies.
/// All copies read the child image, so no overlapping same-texture copy is needed.
pub(crate) fn image_copy_regions(
    placement: ImageResourcePlacement,
) -> impl Iterator<Item = ImageCopyRegion> {
    let mut regions = [ImageCopyRegion::default(); 9];
    let count = match placement {
        ImageResourcePlacement::Texture(rect) => {
            assert!(
                rect.width > 0 && rect.height > 0,
                "image placement must be nonempty"
            );
            regions[0] = ImageCopyRegion {
                source: [0, 0],
                destination: [0, 0, 0],
                extent: [rect.width, rect.height],
            };
            1
        }
        ImageResourcePlacement::Atlas(rect) => {
            assert!(
                rect.width > 0 && rect.height > 0,
                "image placement must be nonempty"
            );
            assert!(
                rect.x > 0 && rect.y > 0,
                "atlas image requires its reserved border"
            );
            let xs = [
                (0, rect.x - 1, 1),
                (0, rect.x, rect.width),
                (rect.width - 1, rect.x + rect.width, 1),
            ];
            let ys = [
                (0, rect.y - 1, 1),
                (0, rect.y, rect.height),
                (rect.height - 1, rect.y + rect.height, 1),
            ];
            for (index, ((sy, dy, height), (sx, dx, width))) in ys
                .into_iter()
                .flat_map(|y| xs.into_iter().map(move |x| (y, x)))
                .enumerate()
            {
                regions[index] = ImageCopyRegion {
                    source: [sx, sy],
                    destination: [dx, dy, rect.page],
                    extent: [width, height],
                };
            }
            9
        }
    };
    regions.into_iter().take(count)
}

pub(crate) struct CachedVectorImage<T> {
    // Pin allocation identity without making external source ownership decide cache
    // lifetime. Weak ownership also makes Rc::make_mut assign an edited scene a new ID.
    _source: Weak<Canvas>,
    retained_epoch: u64,
    pub(crate) value: T,
    #[cfg(feature = "wgpu")]
    pub(crate) ready: ReadyResource,
}

pub(crate) struct VectorImageCache<T> {
    entries: FxHashMap<usize, CachedVectorImage<T>>,
    retained_epoch: u64,
}

impl<T> Default for VectorImageCache<T> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            retained_epoch: 0,
        }
    }
}

impl<T> VectorImageCache<T> {
    pub(crate) fn get_or_insert(
        &mut self,
        source: &Rc<Canvas>,
        create: impl FnOnce() -> T,
    ) -> &mut CachedVectorImage<T> {
        self.entries
            .entry(Rc::as_ptr(source) as usize)
            .or_insert_with(|| CachedVectorImage {
                _source: Rc::downgrade(source),
                retained_epoch: self.retained_epoch,
                value: create(),
                #[cfg(feature = "wgpu")]
                ready: ReadyResource::pending(),
            })
    }

    /// Reconcile against the current graph, including empty/reset scenes. Keeping
    /// a Canvas alive outside the renderer never keeps detached GPU cache entries.
    pub(crate) fn retain_sources<'a>(&mut self, sources: impl IntoIterator<Item = &'a Rc<Canvas>>) {
        if self.entries.is_empty() {
            return;
        }
        self.retained_epoch = self
            .retained_epoch
            .checked_add(1)
            .expect("vector image retention epoch exhausted");
        for source in sources {
            if let Some(entry) = self.entries.get_mut(&(Rc::as_ptr(source) as usize)) {
                entry.retained_epoch = self.retained_epoch;
            }
        }
        self.entries
            .retain(|_, entry| entry.retained_epoch == self.retained_epoch);
    }
}

#[cfg(test)]
mod tests;
