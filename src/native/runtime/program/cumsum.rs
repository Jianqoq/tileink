//! Validation is separate from encoding: GPU scan output may be consumed in the
//! same batch without reading it back to the CPU or changing the shared scene layout.
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::shared::gpu_constants::CUMSUM_CHUNK_SIZE;
use std::borrow::Cow;

pub struct CumsumPlan<'a> {
    offsets: Cow<'a, [u32]>,
    lengths: Cow<'a, [u32]>,
    starts: Cow<'a, [u32]>,
    ends: Cow<'a, [u32]>,
    backdrop_words: usize,
    needs_offsets: bool,
}
pub struct CumsumOutput {
    pub totals: ResourceId,
    pub offsets: ResourceId,
}

fn validate_row_ownership(
    rows: impl IntoIterator<Item = (u32, u32)>,
    lengths: &[u32],
) -> Result<()> {
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
    Ok(())
}
impl CumsumPlan<'static> {
    pub fn new(
        offsets: Vec<u32>,
        lengths: Vec<u32>,
        starts: Vec<u32>,
        ends: Vec<u32>,
        backdrop_words: usize,
    ) -> Result<Self> {
        Self::with_data(
            Cow::Owned(offsets),
            Cow::Owned(lengths),
            Cow::Owned(starts),
            Cow::Owned(ends),
            backdrop_words,
        )
    }
}

impl<'a> CumsumPlan<'a> {
    /// The persistent scene owns these arrays for the entire batch. Borrowing them removes
    /// four per-frame copies while running the same bounds and ownership validation as `new`.
    pub(crate) fn from_shared(
        plan: &'a crate::shared::gpu_plan::GpuCumsumPlan,
        backdrop_words: usize,
    ) -> Result<Self> {
        Self::with_data(
            Cow::Borrowed(&plan.chunk_backdrop_offsets),
            Cow::Borrowed(&plan.chunk_lens),
            Cow::Borrowed(&plan.row_chunk_starts),
            Cow::Borrowed(&plan.row_chunk_ends),
            backdrop_words,
        )
    }

    fn with_data(
        offsets: Cow<'a, [u32]>,
        lengths: Cow<'a, [u32]>,
        starts: Cow<'a, [u32]>,
        ends: Cow<'a, [u32]>,
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
        let mut previous_end = 0u64;
        let mut ordered_spans = true;
        for (&offset, &len) in offsets.iter().zip(lengths.iter()) {
            if len > CUMSUM_CHUNK_SIZE || offset as u64 + len as u64 > backdrop_words as u64 {
                return Err("cumsum chunk out of bounds".into());
            }
            if len != 0 {
                ordered_spans &= offset as u64 >= previous_end;
                previous_end = offset as u64 + len as u64;
            }
        }
        // Arena order is usually already backdrop order. Sort only for a valid but
        // out-of-order caller; the common path still checks every extent.
        if !ordered_spans {
            let mut spans = offsets
                .iter()
                .zip(lengths.iter())
                .filter(|(_, len)| **len != 0)
                .map(|(&offset, &len)| (offset as u64, offset as u64 + len as u64))
                .collect::<Vec<_>>();
            spans.sort_unstable();
            if spans.windows(2).any(|w| w[0].1 > w[1].0) {
                return Err("overlapping cumsum backdrop writes".into());
            }
        }
        let mut previous_start = 0u32;
        let mut ordered_rows = true;
        for (&start, &end) in starts.iter().zip(ends.iter()) {
            if start > end || end as usize > offsets.len() {
                return Err("invalid cumsum row chunk range".into());
            }
            if start != end {
                ordered_rows &= start >= previous_start;
                previous_start = start;
            }
        }
        let rows = starts
            .iter()
            .zip(ends.iter())
            .filter(|(start, end)| start != end)
            .map(|(&start, &end)| (start, end));
        if ordered_rows {
            validate_row_ownership(rows, &lengths)?;
        } else {
            let mut sorted_rows = rows.collect::<Vec<_>>();
            sorted_rows.sort_unstable();
            validate_row_ownership(sorted_rows, &lengths)?;
        }
        // Shared scene arenas retain empty rows/chunks. Table cardinality cannot
        // establish the single-chunk fast path when those holes are present.
        let needs_offsets = starts
            .iter()
            .zip(ends.iter())
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
        self.encode_cached(
            batch,
            backdrops,
            maximum_dimension,
            &mut CumsumBuffers::default(),
            None,
        )
    }

    pub(crate) fn encode_cached(
        &self,
        batch: &mut ComputeBatch,
        backdrops: ResourceId,
        maximum_dimension: u32,
        buffers: &mut CumsumBuffers,
        dirty: Option<&crate::shared::gpu_plan::GpuPathPlanDirty>,
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
        let offsets = buffers.offsets.upload(
            batch,
            &self.offsets,
            dirty.map(|dirty| dirty.cumsum_chunks.as_slice()),
        )?;
        let lengths = buffers.lengths.upload(
            batch,
            &self.lengths,
            dirty.map(|dirty| dirty.cumsum_chunks.as_slice()),
        )?;
        let starts = buffers.starts.upload(
            batch,
            &self.starts,
            dirty.map(|dirty| dirty.cumsum_rows.as_slice()),
        )?;
        let ends = buffers.ends.upload(
            batch,
            &self.ends,
            dirty.map(|dirty| dirty.cumsum_rows.as_slice()),
        )?;
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

#[derive(Default)]
pub(crate) struct CumsumBuffers {
    offsets: super::cached_buffer::CachedBuffer,
    lengths: super::cached_buffer::CachedBuffer,
    starts: super::cached_buffer::CachedBuffer,
    ends: super::cached_buffer::CachedBuffer,
}
