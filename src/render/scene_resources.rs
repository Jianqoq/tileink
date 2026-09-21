//! Offscreen allocation reuse and submission-boundary quarantine, independent
//! of the API objects inside each allocation set.

/// The requested local size follows the lease while its allocation is swapped
/// with the parent's active set, so restoration returns it to the matching pool.
pub(crate) struct SceneResources<T> {
    pub(crate) target_size: (u32, u32),
    pub(crate) allocation: T,
}

pub(crate) struct SceneResourcePool<T> {
    available: Vec<SceneResources<T>>,
    pending: Vec<SceneResources<T>>,
}

impl<T> Default for SceneResourcePool<T> {
    fn default() -> Self {
        Self {
            available: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl<T> SceneResourcePool<T> {
    pub(crate) fn acquire(&mut self, size: (u32, u32)) -> Option<SceneResources<T>> {
        let mut fallback = self.available.pop()?;
        // Nested and repeated scenes naturally use the O(1) LIFO path. Search
        // only when a differently sized sibling left a mismatched tail entry.
        if !self.available.is_empty()
            && fallback.target_size != size
            && let Some(index) = self
                .available
                .iter()
                .rposition(|entry| entry.target_size == size)
        {
            let resources = self.available.swap_remove(index);
            self.available.push(fallback);
            return Some(resources);
        }
        fallback.target_size = size;
        Some(fallback)
    }

    /// No sibling may overwrite queue-write inputs still used by an unsubmitted
    /// batch. Reuse becomes possible only at the next resolved batch boundary.
    pub(crate) fn recycle(&mut self, resources: SceneResources<T>) {
        self.pending.push(resources);
    }

    /// The previous owning batch must already be submitted or discarded. Future
    /// writes must be ordered after its GPU reads. This grants buffer reuse,
    /// not completion: adapters still own retirement of mapped ranges,
    /// descriptors, command allocators and all submitted GPU-use leases.
    pub(crate) fn begin_frame(&mut self) {
        self.available.append(&mut self.pending);
    }

    #[cfg(any(test, all(feature = "wgpu", feature = "bench-internals")))]
    pub(crate) fn clear(&mut self) {
        self.available.clear();
        self.pending.clear();
    }

    #[cfg(test)]
    pub(crate) fn available(&self) -> &[SceneResources<T>] {
        &self.available
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> &[SceneResources<T>] {
        &self.pending
    }
}

#[cfg(test)]
mod tests;
