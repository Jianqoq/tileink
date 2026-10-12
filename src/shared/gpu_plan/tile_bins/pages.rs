use super::*;

impl TileDrawBins {
    #[allow(
        clippy::expect_used,
        reason = "The shrinking loop pops only while the page count exceeds its nonnegative target"
    )]
    pub(super) fn rewrite_tile(&mut self, tile: usize) {
        let cull = self.occlusion.active
            && self.occlusion.filter_tile(
                tile,
                self.tiles_size.0,
                &self.tile_refs[tile],
                &mut self.visible_refs,
            );
        let len = if cull {
            self.visible_refs.len()
        } else {
            self.tile_refs[tile].len()
        };
        let required = len.div_ceil(COARSE_CHUNK_SIZE as usize);
        while self.tile_pages[tile].len() < required {
            let page = self.allocate_page();
            self.tile_pages[tile].push(page);
            self.active_pages += 1;
        }
        while self.tile_pages[tile].len() > required {
            let page = self.tile_pages[tile].pop().expect(
                "The shrinking loop pops only while the page count exceeds its nonnegative target",
            );
            self.free_pages.push(page);
            self.active_pages -= 1;
        }
        self.records[tile] = TileDrawRecord {
            start: self.tile_pages[tile].first().copied().unwrap_or(u32::MAX),
            end: len as u32,
        };
        self.dirty_records.push(tile);
        let refs = if cull {
            &self.visible_refs
        } else {
            &self.tile_refs[tile]
        };
        for (local_page, &page) in self.tile_pages[tile].iter().enumerate() {
            let base = page as usize * TILE_DRAW_PAGE_WORDS;
            self.draw_indices[base..base + TILE_DRAW_PAGE_WORDS].fill(u32::MAX);
            self.draw_indices[base] = self.tile_pages[tile]
                .get(local_page + 1)
                .copied()
                .unwrap_or(u32::MAX);
            let start = local_page * COARSE_CHUNK_SIZE as usize;
            let end = (start + COARSE_CHUNK_SIZE as usize).min(len);
            self.draw_indices[base + 1..base + 1 + end - start].copy_from_slice(&refs[start..end]);
            self.dirty_pages.push(page);
        }
    }

    /// A full upload needs one compact GPU arena. Constructing pages first duplicated both
    /// visibility traversal and storage writes; materialize those pages only for later deltas.
    pub(super) fn build_flat_full_upload(&mut self) {
        self.draw_indices.clear();
        self.active_pages = 0;
        for tile in 0..self.tile_refs.len() {
            let cull = self.occlusion.active
                && self.index_format != IndexFormat::FlatPrefix
                && self.occlusion.filter_tile(
                    tile,
                    self.tiles_size.0,
                    &self.tile_refs[tile],
                    &mut self.visible_refs,
                );
            let refs = if cull {
                &self.visible_refs
            } else {
                &self.tile_refs[tile]
            };
            let start = self.draw_indices.len();
            assert!(start < TILE_DRAW_FLAT_FLAG as usize);
            self.records[tile] = TileDrawRecord {
                start: if refs.is_empty() {
                    u32::MAX
                } else {
                    TILE_DRAW_FLAT_FLAG | start as u32
                },
                end: refs.len() as u32,
            };
            self.draw_indices.extend_from_slice(refs);
            self.active_pages += refs.len().div_ceil(COARSE_CHUNK_SIZE as usize);
        }
        if self.index_format == IndexFormat::FlatPrefix && self.occlusion.active {
            // Keep full index/chunk capacity resident across opacity changes.
            for tile in 0..self.records.len() {
                self.refresh_flat_prefix(tile);
            }
        }
    }

    pub(super) fn materialize_pages(&mut self) {
        self.index_format = IndexFormat::Pages;
        self.flat_rebuild_work = 0;
        self.draw_indices.clear();
        self.free_pages.clear();
        self.active_pages = 0;
        for pages in &mut self.tile_pages {
            pages.clear();
        }
        for tile in 0..self.tile_refs.len() {
            self.rewrite_tile(tile);
        }
        self.full_upload = true;
    }

    /// Compare index words touched, including per-tile records. Sparse UI bins can have only a
    /// few IDs per tile; allocating and uploading 257-word pages on their first edit is much more
    /// expensive than rebuilding a compact flat arena. Promote only after the extra flat work
    /// has amortized that one-time page cost. Sustained sparse motion still reaches local deltas.
    pub(super) fn prefer_flat_update(&mut self) -> bool {
        let (flat, pages) = self.tile_refs.iter().fold(
            (self.records.len(), self.records.len()),
            |(flat, pages), refs| {
                (
                    flat + refs.len(),
                    pages + refs.len().div_ceil(COARSE_CHUNK_SIZE as usize) * TILE_DRAW_PAGE_WORDS,
                )
            },
        );
        let delta =
            (0..self.affected_tiles.len()).fold(self.affected_tiles.len(), |words, index| {
                words
                    + self.tile_refs[self.affected_tiles.get(index)]
                        .len()
                        .div_ceil(COARSE_CHUNK_SIZE as usize)
                        * TILE_DRAW_PAGE_WORDS
            });
        if delta >= flat {
            self.flat_rebuild_work = 0;
            return true;
        }
        self.flat_rebuild_work = self.flat_rebuild_work.saturating_add(flat - delta);
        self.flat_rebuild_work < pages
    }

    /// Keep the complete flat arena resident when only visibility changes. Moving the record's
    /// start avoids rewriting thousands of hidden IDs on opacity toggles and preserves capacity.
    pub(super) fn refresh_flat_prefix(&mut self, tile: usize) {
        let refs = &self.tile_refs[tile];
        if refs.is_empty() {
            return;
        }
        let record = &mut self.records[tile];
        let base = (record.start & !TILE_DRAW_FLAT_FLAG) - (refs.len() as u32 - record.end);
        let skip = self.occlusion.visible_start(tile, self.tiles_size.0, refs);
        record.start = TILE_DRAW_FLAT_FLAG | (base + skip as u32);
        record.end = (refs.len() - skip) as u32;
    }

    pub(super) fn allocate_page(&mut self) -> u32 {
        if let Some(page) = self.free_pages.pop() {
            return page;
        }
        let page = (self.draw_indices.len() / TILE_DRAW_PAGE_WORDS) as u32;
        self.draw_indices
            .resize(self.draw_indices.len() + TILE_DRAW_PAGE_WORDS, u32::MAX);
        page
    }

    pub(super) fn maybe_compact_pages(&mut self) {
        let total = self.draw_indices.len() / TILE_DRAW_PAGE_WORDS;
        if total < 16 || self.free_pages.len() * 10 <= total * 3 {
            return;
        }
        self.materialize_pages();
    }

    pub(crate) fn take_dirty(&mut self) -> (bool, Vec<usize>, Vec<u32>) {
        let full = std::mem::take(&mut self.full_upload);
        self.dirty_records.sort_unstable();
        self.dirty_records.dedup();
        self.dirty_pages.sort_unstable();
        self.dirty_pages.dedup();
        (
            full,
            std::mem::take(&mut self.dirty_records),
            std::mem::take(&mut self.dirty_pages),
        )
    }

    /// Returns upload-consumed dirty-list storage so retained updates reuse its capacity.
    pub(crate) fn recycle_dirty(&mut self, mut records: Vec<usize>, mut pages: Vec<u32>) {
        debug_assert!(self.dirty_records.is_empty());
        debug_assert!(self.dirty_pages.is_empty());
        records.clear();
        pages.clear();
        if records.capacity() <= RETAINED_TILE_DIRTY_CAPACITY {
            self.dirty_records = records;
        }
        if pages.capacity() <= RETAINED_TILE_DIRTY_CAPACITY {
            self.dirty_pages = pages;
        }
    }

    pub(crate) const fn active_page_count(&self) -> usize {
        self.active_pages
    }

    pub(crate) fn upload_records(&self) -> &[TileDrawRecord] {
        &self.records
    }

    pub(crate) fn upload_indices(&self) -> &[u32] {
        &self.draw_indices
    }

    pub(crate) fn upload_index_count(&self) -> usize {
        self.upload_indices().len()
    }
}
