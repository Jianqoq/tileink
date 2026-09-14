//! Validation is separate from encoding: GPU scan output may be consumed in the
//! same batch without reading it back to the CPU or changing the shared scene layout.
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::shared::gpu_constants::CUMSUM_CHUNK_SIZE;

pub struct CumsumPlan {
    offsets: Vec<u32>,
    lengths: Vec<u32>,
    starts: Vec<u32>,
    ends: Vec<u32>,
    backdrop_words: usize,
    needs_offsets: bool,
}
pub struct CumsumOutput {
    pub totals: ResourceId,
    pub offsets: ResourceId,
}
impl CumsumPlan {
    pub fn new(
        offsets: Vec<u32>,
        lengths: Vec<u32>,
        starts: Vec<u32>,
        ends: Vec<u32>,
        backdrop_words: usize,
    ) -> Result<Self> {
        if offsets.len() != lengths.len()
            || starts.len() != ends.len()
            || offsets.len() > u32::MAX as usize / 4
            || starts.len() > 65535 * CUMSUM_CHUNK_SIZE as usize
            || backdrop_words > u32::MAX as usize / 4
        {
            return Err("invalid cumsum metadata dimensions".into());
        }
        let mut spans = Vec::new();
        for (&offset, &len) in offsets.iter().zip(&lengths) {
            if len > CUMSUM_CHUNK_SIZE || offset as u64 + len as u64 > backdrop_words as u64 {
                return Err("cumsum chunk out of bounds".into());
            }
            if len != 0 {
                spans.push((offset as u64, offset as u64 + len as u64));
            }
        }
        spans.sort_unstable();
        if spans.windows(2).any(|w| w[0].1 > w[1].0) {
            return Err("overlapping cumsum backdrop writes".into());
        }
        let mut rows = Vec::new();
        for (&start, &end) in starts.iter().zip(&ends) {
            if start > end || end as usize > offsets.len() {
                return Err("invalid cumsum row chunk range".into());
            }
            if start != end {
                rows.push((start, end));
            }
        }
        rows.sort_unstable();
        let mut next = 0;
        for (start, end) in rows {
            if start < next
                || lengths[next as usize..start as usize]
                    .iter()
                    .any(|&len| len != 0)
            {
                return Err("cumsum rows overlap or omit live chunks".into());
            }
            next = end;
        }
        if lengths[next as usize..].iter().any(|&len| len != 0) {
            return Err("cumsum chunks missing row ownership".into());
        }
        // Shared scene arenas retain empty rows/chunks. Table cardinality cannot
        // establish the single-chunk fast path when those holes are present.
        let needs_offsets = starts
            .iter()
            .zip(&ends)
            .any(|(&start, &end)| end - start > 1);
        Ok(Self {
            offsets,
            lengths,
            starts,
            ends,
            backdrop_words,
            needs_offsets,
        })
    }
    pub fn encode(
        &self,
        batch: &mut ComputeBatch,
        backdrops: ResourceId,
        maximum_dimension: u32,
    ) -> Result<Option<CumsumOutput>> {
        if batch.size(backdrops)? < self.backdrop_words * 4 {
            return Err("undersized cumsum backdrop allocation".into());
        }
        if self.offsets.is_empty() {
            return Ok(None);
        }
        if maximum_dimension == 0 || maximum_dimension > 65535 {
            return Err("invalid cumsum dispatch limit".into());
        }
        let chunks = self.offsets.len() as u32;
        let x = chunks.min(maximum_dimension);
        let y = chunks.div_ceil(x);
        if y > maximum_dimension
            || (self.starts.len() as u32).div_ceil(CUMSUM_CHUNK_SIZE) > maximum_dimension
        {
            return Err("cumsum dispatch exceeds device grid".into());
        }
        let grid = [x, y, 1];
        let add = |batch: &mut ComputeBatch, words: &[u32]| {
            batch.buffer(if words.is_empty() {
                vec![0; 4]
            } else {
                words.iter().flat_map(|w| w.to_le_bytes()).collect()
            })
        };
        let config = add(batch, &[self.starts.len() as u32, chunks, 0, 0])?;
        let offsets = add(batch, &self.offsets)?;
        let lengths = add(batch, &self.lengths)?;
        let starts = add(batch, &self.starts)?;
        let ends = add(batch, &self.ends)?;
        let totals = batch.buffer(vec![0; chunks as usize * 4])?;
        let carries = batch.buffer(vec![0; chunks as usize * 4])?;
        // SAFETY: construction validates disjoint backdrop writes, bounded chunk lanes,
        // bounded metadata and exactly one row owner per live chunk; unused chunks have zero length. The native shader
        // guards padded groups using the actual chunk_count in the uniform.
        unsafe {
            batch.dispatch(
                "cumsum_prefix_chunks",
                &[
                    (0, config),
                    (1, offsets),
                    (2, lengths),
                    (5, backdrops),
                    (6, totals),
                ],
                grid,
            )?;
            if self.needs_offsets {
                batch.dispatch(
                    "cumsum_chunk_offsets",
                    &[
                        (0, config),
                        (3, starts),
                        (4, ends),
                        (6, totals),
                        (7, carries),
                    ],
                    [(self.starts.len() as u32).div_ceil(CUMSUM_CHUNK_SIZE), 1, 1],
                )?;
                batch.dispatch(
                    "cumsum_apply_chunk_offsets",
                    &[
                        (0, config),
                        (1, offsets),
                        (2, lengths),
                        (5, backdrops),
                        (7, carries),
                    ],
                    grid,
                )?;
            }
        }
        Ok(Some(CumsumOutput {
            totals,
            offsets: carries,
        }))
    }
}
#[cfg(test)]
#[path = "../tests/cumsum.rs"]
mod tests;
