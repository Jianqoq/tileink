use super::*;

impl TileDrawBins {
    pub(super) fn rewrite_tile(&mut self, tile: usize) {
        let required = self.tile_refs[tile]
            .len()
            .div_ceil(COARSE_CHUNK_SIZE as usize);
        while self.tile_pages[tile].len() < required {
            let page = self.allocate_page();
            self.tile_pages[tile].push(page);
            self.active_pages += 1;
        }
        while self.tile_pages[tile].len() > required {
            let page = self.tile_pages[tile].pop().unwrap();
            self.free_pages.push(page);
            self.active_pages -= 1;
        }
        self.records[tile] = TileDrawRecord {
            start: self.tile_pages[tile].first().copied().unwrap_or(u32::MAX),
            end: self.tile_refs[tile].len() as u32,
        };
        self.dirty_records.push(tile);
        for (local_page, &page) in self.tile_pages[tile].iter().enumerate() {
            let base = page as usize * TILE_DRAW_PAGE_WORDS;
            self.draw_indices[base..base + TILE_DRAW_PAGE_WORDS].fill(u32::MAX);
            self.draw_indices[base] = self.tile_pages[tile]
                .get(local_page + 1)
                .copied()
                .unwrap_or(u32::MAX);
            let start = local_page * COARSE_CHUNK_SIZE as usize;
            let end = (start + COARSE_CHUNK_SIZE as usize).min(self.tile_refs[tile].len());
            self.draw_indices[base + 1..base + 1 + end - start]
                .copy_from_slice(&self.tile_refs[tile][start..end]);
            self.dirty_pages.push(page);
        }
    }

    pub(super) fn build_flat_full_upload(&mut self) {
        self.flat_full_upload = true;
        self.full_upload_records.clear();
        self.full_upload_records.resize(
            self.tile_refs.len(),
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.full_upload_indices.clear();
        for (tile, draws) in self.tile_refs.iter().enumerate() {
            if draws.is_empty() {
                continue;
            }
            let start = self.full_upload_indices.len();
            assert!(start < TILE_DRAW_FLAT_FLAG as usize);
            self.full_upload_records[tile] = TileDrawRecord {
                start: TILE_DRAW_FLAT_FLAG | start as u32,
                end: draws.len() as u32,
            };
            self.full_upload_indices.extend_from_slice(draws);
        }
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
        self.flat_full_upload = false;
        self.full_upload_records.clear();
        self.full_upload_indices.clear();
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

    pub(crate) fn active_page_count(&self) -> usize {
        self.active_pages
    }

    pub(crate) fn upload_records(&self) -> &[TileDrawRecord] {
        if self.flat_full_upload {
            &self.full_upload_records
        } else {
            &self.records
        }
    }

    pub(crate) fn upload_indices(&self) -> &[u32] {
        if self.flat_full_upload {
            &self.full_upload_indices
        } else {
            &self.draw_indices
        }
    }

    pub(crate) fn upload_index_count(&self) -> usize {
        self.upload_indices().len()
    }
}
