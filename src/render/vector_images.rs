//! Deferred vector-image identity and exact GPU copy planning.

use crate::{Canvas, shared::image_resource::AtlasRect};
use rustc_hash::FxHashMap;
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageCopyRegion {
    pub(crate) source: [u32; 2],
    pub(crate) destination: [u32; 3],
    pub(crate) extent: [u32; 2],
}

/// Reproduce the raster uploader's one-pixel clamp border using texture copies.
/// All copies read the child image, so no overlapping same-texture copy is needed.
pub(crate) fn atlas_copy_regions(rect: AtlasRect) -> [ImageCopyRegion; 9] {
    assert!(
        rect.width > 0 && rect.height > 0,
        "atlas image must be nonempty"
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
    std::array::from_fn(|index| {
        let (sx, dx, width) = xs[index % 3];
        let (sy, dy, height) = ys[index / 3];
        ImageCopyRegion {
            source: [sx, sy],
            destination: [dx, dy, rect.page],
            extent: [width, height],
        }
    })
}

pub(crate) struct CachedVectorImage<T> {
    // Pin allocation identity without making external source ownership decide cache
    // lifetime. Weak ownership also makes Rc::make_mut assign an edited scene a new ID.
    _source: Weak<Canvas>,
    retained_epoch: u64,
    pub(crate) value: T,
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
            })
    }

    /// Reconcile against the current graph, including empty/reset scenes. Keeping
    /// a Canvas alive outside the renderer never keeps detached GPU cache entries.
    #[allow(
        clippy::expect_used,
        reason = "Exhausting the retention epoch must fail rather than reuse a live cache identity"
    )]
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
