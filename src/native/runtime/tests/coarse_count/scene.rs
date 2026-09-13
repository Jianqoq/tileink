use super::*;

pub(super) fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

pub(super) fn count_scene(draw_count: u32, linked: bool, stack: u32) -> CountScene {
    // One active tile with a variable-length paged list. Alternating kinds cover
    // SDF/shadow, winding/segments, vector glyphs, clips, ignored tags and batches.
    let pattern = [1u32, 1, 1, 1, 0, 0, 1, 0];
    let mut draws = Vec::new();
    for index in 0..8 {
        let mut draw = [0u32; 31];
        draw[0] = 0;
        draw[1] = u32::MAX;
        draw[2] = u32::MAX;
        draw[4] = u32::MAX;
        draw[12] = 16;
        draw[13] = 16;
        draw[19] = 1f32.to_bits();
        draw[22] = 1f32.to_bits();
        match index {
            0 => draw[2] = 0,
            1 => draw[4] = 0,
            3 => draw[8] = 5,
            4 => draw[8] = 2,
            6 => draw[8] = 1,
            7 => draw[0] = u32::MAX,
            _ => (),
        }
        draws.extend(draw);
    }
    let pages = draw_count.div_ceil(COARSE_WORKGROUP_SIZE);
    let indices = if linked {
        pages * (COARSE_WORKGROUP_SIZE + 1)
    } else {
        draw_count
    };
    let draw_base = 6 + 3 * 6 + 5;
    let index_base = draw_base + 2;
    let tile_emit_base = index_base + indices;
    let emit_base = tile_emit_base + 2;
    let kind_base = emit_base + (pages + 2) * 7;
    let mut work = vec![0x45454545; (kind_base + 9) as usize];
    work[draw_base as usize] = if draw_count == 0 {
        u32::MAX
    } else if linked {
        0
    } else {
        0x80000000
    };
    work[draw_base as usize + 1] = draw_count;
    for index in 0..draw_count {
        let word = if linked {
            index / COARSE_WORKGROUP_SIZE * (COARSE_WORKGROUP_SIZE + 1)
                + 1
                + index % COARSE_WORKGROUP_SIZE
        } else {
            index
        };
        work[(index_base + word) as usize] = index % 8;
    }
    for page in 0..pages {
        if linked {
            work[(index_base + page * (COARSE_WORKGROUP_SIZE + 1)) as usize] =
                if page + 1 < pages { page + 1 } else { u32::MAX };
        }
        let base = (emit_base + page * 7) as usize;
        work[base] = 0;
        work[base + 1] = page;
    }
    work[tile_emit_base as usize] = pages;
    work[tile_emit_base as usize + 1] = 0;
    let mut expected = work.clone();
    let mut particles: u32 = (0..draw_count).map(|i| pattern[(i % 8) as usize]).sum();
    // stack: 0 absent; 1 full path clip elided; 2 opacity retained; 3 invalid tag.
    let wrapper = u32::from(stack == 2);
    if stack == 3 {
        particles = 0;
    }
    expected[0] = if particles == 0 {
        0
    } else {
        particles + wrapper * 2 + 1
    };
    expected[3] = 0;
    expected[kind_base as usize] = u32::from(particles == 0);
    let mut config = [0u32; 19];
    config[0] = 1;
    config[1] = 1;
    config[2] = 1;
    config[3] = 42;
    config[6] = u32::from(stack != 0);
    config[7] = 3;
    config[8] = 5;
    config[12] = indices;
    config[13] = pages + 2;
    config[16] = 1;
    let mut path = [0u32; 19];
    path[8] = 1;
    path[9] = 1;
    let layers = [
        match stack {
            1 => 0,
            2 => 1,
            _ => 99,
        },
        2,
        0,
    ];
    let batches = [42, 42, 42, 42, 42, 99, 42, 42];
    CountScene {
        config,
        draws,
        text: vec![0],
        paths: path.to_vec(),
        backdrops: vec![1],
        ranges: vec![0, 0],
        layers: layers.to_vec(),
        work,
        sdf: vec![0; 9],
        batches: batches.to_vec(),
        expected,
        kind_base: kind_base as usize,
    }
}

pub(super) struct CountScene {
    pub(super) config: [u32; 19],
    pub(super) draws: Vec<u32>,
    pub(super) text: Vec<u32>,
    pub(super) paths: Vec<u32>,
    pub(super) backdrops: Vec<u32>,
    pub(super) ranges: Vec<u32>,
    pub(super) layers: Vec<u32>,
    pub(super) work: Vec<u32>,
    pub(super) sdf: Vec<u32>,
    pub(super) batches: Vec<u32>,
    pub(super) expected: Vec<u32>,
    pub(super) kind_base: usize,
}
impl CountScene {
    pub(super) fn batch(&self, entry: &'static str) -> Result<ComputeBatch> {
        self.batch_grid(entry, [2, 1, 1])
    }
    pub(super) fn batch_grid(&self, entry: &'static str, grid: [u32; 3]) -> Result<ComputeBatch> {
        let mut batch = ComputeBatch::new();
        let data: [&[u32]; 10] = [
            &self.config,
            &self.draws,
            &self.text,
            &self.paths,
            &self.backdrops,
            &self.ranges,
            &self.layers,
            &self.work,
            &self.sdf,
            &self.batches,
        ];
        let bindings = data
            .iter()
            .enumerate()
            .map(|(slot, words)| Ok((slot as u32, batch.buffer(bytes(words))?)))
            .collect::<Result<Vec<_>>>()?;
        // SAFETY: every draw/page/path/stack reference is initialized and bounded;
        // padded groups/lanes must return before raw memory access or barriers.
        unsafe {
            batch.dispatch(entry, &bindings, grid)?;
        }
        batch.readback(bindings[7].1)?;
        Ok(batch)
    }
    pub(super) fn tile_counts_batch(&self) -> Result<ComputeBatch> {
        let mut batch = ComputeBatch::new();
        let data: [(u32, &[u32]); 8] = [
            (0, &self.config),
            (1, &self.draws),
            (3, &self.paths),
            (4, &self.backdrops),
            (5, &self.ranges),
            (6, &self.layers),
            (7, &self.work),
            (9, &self.sdf),
        ];
        let bindings = data
            .iter()
            .map(|(slot, words)| Ok((*slot, batch.buffer(bytes(words))?)))
            .collect::<Result<Vec<_>>>()?;
        // SAFETY: initialized tile ranges and scene records; extra lanes exercise the tile guard.
        unsafe {
            batch.dispatch(
                "coarse_tile_counts_from_emit_chunks",
                &bindings,
                [self.config[0].div_ceil(COARSE_WORKGROUP_SIZE) + 1, 1, 1],
            )?;
        }
        batch.readback(bindings[6].1)?;
        Ok(batch)
    }
    pub(super) fn particle_counts_batch(&self) -> Result<ComputeBatch> {
        let mut batch = ComputeBatch::new();
        let data: [(u32, &[u32]); 10] = [
            (0, &self.config),
            (1, &self.draws),
            (2, &self.text),
            (3, &self.paths),
            (4, &self.backdrops),
            (5, &self.ranges),
            (6, &self.layers),
            (7, &self.work),
            (9, &self.sdf),
            (10, &self.batches),
        ];
        let bindings = data
            .iter()
            .map(|(slot, words)| Ok((*slot, batch.buffer(bytes(words))?)))
            .collect::<Result<Vec<_>>>()?;
        let chunks = self.config[13] - 2;
        // SAFETY: the final two capacity records are deliberately initialized stale
        // references. Logical dispatch bounds must exclude them before any write.
        unsafe {
            batch.dispatch(
                "coarse_emit_chunk_particle_counts",
                &bindings,
                [2, chunks.div_ceil(2).max(1), 1],
            )?;
        }
        batch.readback(bindings[7].1)?;
        Ok(batch)
    }
    pub(super) fn tile_kinds_batch(&self) -> Result<ComputeBatch> {
        let mut batch = ComputeBatch::new();
        let data: [(u32, &[u32]); 8] = [
            (0, &self.config),
            (1, &self.draws),
            (3, &self.sdf),
            (4, &self.paths),
            (5, &self.backdrops),
            (6, &self.ranges),
            (7, &self.layers),
            (8, &self.work),
        ];
        let bindings = data
            .iter()
            .map(|(slot, words)| Ok((*slot, batch.buffer(bytes(words))?)))
            .collect::<Result<Vec<_>>>()?;
        // SAFETY: initialized tile/reference ranges; padded threads must leave sentinels intact.
        unsafe {
            batch.dispatch(
                "coarse_emit_chunk_tile_kinds",
                &bindings,
                [self.config[0].div_ceil(COARSE_WORKGROUP_SIZE) + 1, 1, 1],
            )?;
        }
        batch.readback(bindings[7].1)?;
        Ok(batch)
    }
    pub(super) fn expect_counts(&mut self, particles: u32, glyphs: u32) {
        self.expected = self.work.clone();
        self.expected[0] = particles;
        self.expected[3] = glyphs;
        self.expected[self.kind_base] = u32::from(particles == 0);
    }
}
