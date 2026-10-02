use crate::shared::{gpu_coarse::*, gpu_plan::GpuBufferLengths};

/// Prove all monotonic packed offsets fit before the existing offset helpers run.
/// This covers host usize overflow and HLSL's u32 byte addresses, including the
/// trailing active-tile list. Record strides come from the canonical host layouts.
pub(crate) fn validate_work_layout(lengths: GpuBufferLengths) -> Result<usize, &'static str> {
    let mut words = 0usize;
    for (count, stride) in [
        (
            lengths.tile_count,
            TILE_COARSE_RECORD_WORDS
                + TILE_DRAW_RECORD_WORDS
                + TILE_EMIT_CHUNK_RECORD_WORDS
                + FINE_TILE_KIND_WORDS
                + 1,
        ),
        (lengths.coarse_ptcl_capacity, PTCL_RECORD_WORDS),
        (lengths.coarse_glyph_capacity, 1),
        (lengths.tile_draw_index_count, 1),
        (lengths.tile_draw_chunk_count, EMIT_CHUNK_RECORD_WORDS),
    ] {
        words = count
            .checked_mul(stride)
            .and_then(|n| words.checked_add(n))
            .filter(|n| *n <= u32::MAX as usize / size_of::<u32>())
            .ok_or("coarse work exceeds raw-buffer address space")?;
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_covers_the_complete_shared_coarse_layout() {
        for count in [0, 1, 3, 257] {
            let lengths = GpuBufferLengths {
                tile_count: count,
                coarse_ptcl_capacity: count * 5,
                coarse_glyph_capacity: count * 7,
                tile_draw_index_count: count * 11,
                tile_draw_chunk_count: count * 13,
                ..Default::default()
            };
            assert_eq!(
                validate_work_layout(lengths).unwrap(),
                coarse_work_word_len(count, count * 5, count * 7, count * 11, count * 13)
            );
        }
        for count in [u32::MAX as usize, usize::MAX] {
            for lengths in [
                GpuBufferLengths {
                    tile_count: count,
                    ..Default::default()
                },
                GpuBufferLengths {
                    coarse_ptcl_capacity: count,
                    ..Default::default()
                },
                GpuBufferLengths {
                    coarse_glyph_capacity: count,
                    ..Default::default()
                },
                GpuBufferLengths {
                    tile_draw_index_count: count,
                    ..Default::default()
                },
                GpuBufferLengths {
                    tile_draw_chunk_count: count,
                    ..Default::default()
                },
            ] {
                assert!(validate_work_layout(lengths).is_err());
            }
        }
    }
}
