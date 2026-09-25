use std::collections::{HashMap, HashSet};

use crate::{NodeGeneration, RetainedNodeId, canvas::RetainedSurfaceId, shared::bounds::Bounds};

/// Allocation accounting for the retained-surface budget.
///
/// The adapter reports allocated storage, including capacity kept after a resize.
/// Dropping this owner only removes a cache reference; the adapter must retain any
/// outstanding GPU-use leases until their submissions have completed.
pub(crate) trait SurfaceAllocation {
    fn byte_len(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetainedSurfaceKind {
    Group,
    Filter,
    Backdrop,
    Mask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RetainedSurfaceMeta {
    pub(crate) revision: NodeGeneration,
    pub(crate) kind: RetainedSurfaceKind,
    pub(crate) size: (u32, u32),
    pub(crate) origin: (i32, i32),
    pub(crate) bounds: Bounds,
}

pub(crate) struct RetainedSurface<T> {
    pub(crate) meta: RetainedSurfaceMeta,
    /// Cached composited output for every surface kind.
    pub(crate) primary: T,
    /// Group/mask clip mask, filter source history, or backdrop clip mask.
    pub(crate) secondary: Option<T>,
    /// Pixels captured immediately before a backdrop layer is composited.
    ///
    /// The root history contains the final previous frame, including content
    /// drawn after the backdrop. Sampling it during a dirty-tile rerender feeds
    /// that foreground back into blur/refraction at clean tile boundaries.
    /// Backdrop source history preserves the correct painter-order input.
    pub(crate) backdrop_source: Option<T>,
    last_used: u64,
}

impl<T: SurfaceAllocation> RetainedSurface<T> {
    fn byte_len(&self) -> u64 {
        self.primary.byte_len()
            + self.secondary.as_ref().map_or(0, T::byte_len)
            + self.backdrop_source.as_ref().map_or(0, T::byte_len)
    }
}

pub(crate) struct RetainedSurfaceCache<T> {
    entries: HashMap<RetainedSurfaceId, RetainedSurface<T>>,
    byte_len: u64,
    budget: u64,
    clock: u64,
    backdrop_evicted: bool,
    protected_before: Option<u64>,
}

impl<T: SurfaceAllocation> RetainedSurfaceCache<T> {
    pub(crate) fn new(budget: u64) -> Self {
        Self {
            entries: HashMap::new(),
            byte_len: 0,
            budget,
            clock: 0,
            backdrop_evicted: false,
            protected_before: None,
        }
    }

    pub(crate) fn set_budget(&mut self, budget: u64) {
        assert!(
            self.protected_before.is_none(),
            "change the surface budget between partial frames"
        );
        self.budget = budget;
        self.evict_to_budget();
    }

    pub(crate) fn take(&mut self, id: RetainedSurfaceId) -> Option<RetainedSurface<T>> {
        let surface = self.entries.remove(&id)?;
        self.byte_len = self.byte_len.saturating_sub(surface.byte_len());
        Some(surface)
    }

    /// Reports whether painter-order source history was lost since the last
    /// check. The renderer must use a full root redraw before rebuilding such
    /// a backdrop; a partial root contains later foreground in its clean tiles.
    pub(crate) fn take_backdrop_evicted(&mut self) -> bool {
        std::mem::take(&mut self.backdrop_evicted)
    }

    pub(crate) fn insert(
        &mut self,
        id: RetainedSurfaceId,
        meta: RetainedSurfaceMeta,
        primary: T,
        secondary: Option<T>,
        backdrop_source: Option<T>,
    ) {
        self.advance_clock();
        let surface = RetainedSurface {
            meta,
            primary,
            secondary,
            backdrop_source,
            last_used: self.clock,
        };
        let bytes = surface.byte_len();
        if bytes > self.budget {
            // Root-cause fix: a replacement supersedes the previous pixels even when
            // its new allocation cannot fit. Keeping the old entry would make stale
            // content reusable after raster-only invalidation with unchanged metadata.
            self.take(id);
            self.backdrop_evicted |= meta.kind == RetainedSurfaceKind::Backdrop;
            return;
        }
        if let Some(old) = self.entries.insert(id, surface) {
            self.byte_len = self.byte_len.saturating_sub(old.byte_len());
        }
        self.byte_len = self.byte_len.saturating_add(bytes);
        self.evict_to_budget();
    }

    pub(crate) fn retain_nodes(&mut self, nodes: &HashSet<RetainedNodeId>) {
        self.entries.retain(|id, surface| {
            let keep = nodes.contains(&id.node);
            if !keep {
                self.byte_len = self.byte_len.saturating_sub(surface.byte_len());
            }
            keep
        });
    }

    pub(crate) fn remove_nodes(&mut self, nodes: &HashSet<RetainedNodeId>) {
        if nodes.is_empty() {
            return;
        }
        self.entries.retain(|id, surface| {
            let keep = !nodes.contains(&id.node);
            if !keep {
                self.byte_len = self.byte_len.saturating_sub(surface.byte_len());
            }
            keep
        });
    }

    /// A partial root contains final old pixels, so unvisited backdrop input
    /// cannot be reconstructed if an earlier insertion evicts it mid-frame.
    /// A clock boundary protects that history in O(1), independent of temporary
    /// active-work suspension inside nested effects. Full redraws retain normal LRU.
    pub(crate) fn begin_frame(&mut self, partial: bool) {
        self.protected_before = partial.then_some(self.clock);
    }

    pub(crate) fn end_frame(&mut self) {
        self.protected_before = None;
    }

    fn advance_clock(&mut self) {
        if self.clock == u64::MAX {
            // Rebase only on overflow; wrapping would make fresh entries look
            // oldest and cross the current frame's protected-history boundary.
            let cutoff = self.protected_before;
            let mut entries = self.entries.values_mut().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|surface| surface.last_used);
            self.clock = 0;
            self.protected_before = cutoff.map(|_| 0);
            for surface in entries {
                let protected = cutoff.is_some_and(|cutoff| surface.last_used <= cutoff);
                self.clock += 1;
                surface.last_used = self.clock;
                if protected {
                    self.protected_before = Some(self.clock);
                }
            }
        }
        self.clock += 1;
    }

    fn evict_to_budget(&mut self) {
        while self.byte_len > self.budget {
            let Some(id) = self
                .entries
                .iter()
                .filter(|(_, surface)| {
                    surface.meta.kind != RetainedSurfaceKind::Backdrop
                        || self
                            .protected_before
                            .is_none_or(|cutoff| surface.last_used > cutoff)
                })
                .min_by_key(|(_, surface)| surface.last_used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            if let Some(surface) = self.entries.remove(&id) {
                self.byte_len = self.byte_len.saturating_sub(surface.byte_len());
                self.backdrop_evicted |= surface.meta.kind == RetainedSurfaceKind::Backdrop;
            }
        }
    }
}

#[cfg(test)]
mod tests;
