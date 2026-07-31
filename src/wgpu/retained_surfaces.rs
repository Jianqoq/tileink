use std::collections::{HashMap, HashSet};

use crate::{NodeGeneration, RetainedNodeId, canvas::RetainedSurfaceId, shared::bounds::Bounds};

use super::target::WgpuTarget;

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

pub(crate) struct RetainedSurface {
    pub(crate) meta: RetainedSurfaceMeta,
    /// Cached composited output for every surface kind.
    pub(crate) primary: WgpuTarget,
    /// Group/mask clip mask, filter source history, or backdrop clip mask.
    pub(crate) secondary: Option<WgpuTarget>,
    /// Pixels captured immediately before a backdrop layer is composited.
    ///
    /// The root history contains the final previous frame, including content
    /// drawn after the backdrop. Sampling it during a dirty-tile rerender feeds
    /// that foreground back into blur/refraction at clean tile boundaries.
    /// Backdrop source history preserves the correct painter-order input.
    pub(crate) backdrop_source: Option<WgpuTarget>,
    last_used: u64,
}

impl RetainedSurface {
    fn byte_len(&self) -> u64 {
        self.primary.byte_len()
            + self.secondary.as_ref().map_or(0, WgpuTarget::byte_len)
            + self
                .backdrop_source
                .as_ref()
                .map_or(0, WgpuTarget::byte_len)
    }
}

pub(crate) struct RetainedSurfaceCache {
    entries: HashMap<RetainedSurfaceId, RetainedSurface>,
    byte_len: u64,
    budget: u64,
    clock: u64,
    backdrop_evicted: bool,
}

impl RetainedSurfaceCache {
    pub(crate) fn new(budget: u64) -> Self {
        Self {
            entries: HashMap::new(),
            byte_len: 0,
            budget,
            clock: 0,
            backdrop_evicted: false,
        }
    }

    pub(crate) fn set_budget(&mut self, budget: u64) {
        self.budget = budget;
        self.evict_to_budget();
    }

    pub(crate) fn take(&mut self, id: RetainedSurfaceId) -> Option<RetainedSurface> {
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
        primary: WgpuTarget,
        secondary: Option<WgpuTarget>,
        backdrop_source: Option<WgpuTarget>,
    ) {
        self.clock = self.clock.wrapping_add(1);
        let surface = RetainedSurface {
            meta,
            primary,
            secondary,
            backdrop_source,
            last_used: self.clock,
        };
        let bytes = surface.byte_len();
        if bytes > self.budget {
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

    fn evict_to_budget(&mut self) {
        while self.byte_len > self.budget {
            let Some(id) = self
                .entries
                .iter()
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
