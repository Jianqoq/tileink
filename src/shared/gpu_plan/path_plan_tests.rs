use super::*;
use crate::{Canvas, FillRule};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

#[test]
fn scan_plan_records_are_gpu_word_layouts() {
    assert_eq!(std::mem::size_of::<GpuScanChunk>(), 16);
    assert_eq!(std::mem::align_of::<GpuScanChunk>(), 4);
    assert_eq!(std::mem::size_of::<GpuScanChunkRange>(), 8);
    assert_eq!(std::mem::align_of::<GpuScanChunkRange>(), 4);
}

#[test]
fn scan_chunks_cover_each_backdrop_record_in_fixed_size_tiles() {
    let mut canvas = Canvas::new(
        (SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE,
        crate::TILE_SIZE,
        1.0,
    );
    canvas.push_path(
        Rect::new(
            0.0,
            0.0,
            f64::from((SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE),
            f64::from(crate::TILE_SIZE),
        )
        .to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let lengths = GpuBufferLengths::from_scene(&canvas);
    let (chunks, ranges) = build_scan_chunks(&canvas);

    assert_eq!(lengths.scan_chunk_count, 2);
    assert_eq!(chunks.len(), 2);
    assert_eq!(ranges[0].start, 0);
    assert_eq!(ranges[0].end, 2);
    assert_eq!(chunks[0].backdrop_offset, 0);
    assert_eq!(chunks[0].len, SCAN_CHUNK_SIZE);
    assert_eq!(chunks[1].backdrop_offset, SCAN_CHUNK_SIZE);
    assert_eq!(chunks[1].len, 17);
}

#[test]
fn persistent_path_plans_rebuild_only_changed_path_allocations() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE, 1.0);
    for x in [0.0, 32.0] {
        canvas.push_path(
            Rect::new(x, 0.0, x + 32.0, 16.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    let mut plans = PersistentPathPlans::default();
    plans.update(&canvas, None);
    plans.take_dirty();
    let first = plans.scan_ranges()[0];

    canvas.path_records[1].data_offset += 7;
    plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));
    let dirty = plans.take_dirty();

    assert_eq!(plans.scan_ranges()[0], first);
    assert!(
        dirty
            .scan_chunks
            .iter()
            .all(|range| range.start >= first.end as usize)
    );
    assert_eq!(dirty.scan_ranges, Vec::<std::ops::Range<usize>>::new());
}

#[test]
fn persistent_path_plan_recycles_all_dirty_range_capacities() {
    let mut plans = PersistentPathPlans::default();
    for index in 0..64 {
        let range = index * 2..index * 2 + 1;
        plans.dirty_scan_chunks.push(range.clone());
        plans.dirty_scan_ranges.push(range.clone());
        plans.dirty_cumsum_chunks.push(range.clone());
        plans.dirty_cumsum_rows.push(range);
    }
    let dirty = plans.take_dirty();
    let capacities = [
        dirty.scan_chunks.capacity(),
        dirty.scan_ranges.capacity(),
        dirty.cumsum_chunks.capacity(),
        dirty.cumsum_rows.capacity(),
    ];

    plans.recycle_dirty(dirty);

    assert_eq!(
        [
            plans.dirty_scan_chunks.capacity(),
            plans.dirty_scan_ranges.capacity(),
            plans.dirty_cumsum_chunks.capacity(),
            plans.dirty_cumsum_rows.capacity(),
        ],
        capacities
    );
}

#[test]
fn regrown_vacant_scan_slots_are_uploaded_over_retained_gpu_contents() {
    let mut canvas = Canvas::new(64, 16, 1.0);
    for x in [0.0, 32.0] {
        canvas.push_path(
            Rect::new(x, 0.0, x + 32.0, 16.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    let mut plans = PersistentPathPlans::default();
    plans.update(&canvas, None);
    plans.take_dirty();
    let mut gpu = plans.scan_ranges().to_vec();
    assert_ne!(gpu[1], GpuScanChunkRange::default());
    canvas.path_records.truncate(1);
    plans.update(&canvas, Some(&[]));
    plans.take_dirty();
    canvas.path_records.push(Default::default());
    plans.update(&canvas, Some(&[]));
    for range in plans.take_dirty().scan_ranges {
        gpu[range.clone()].copy_from_slice(&plans.scan_ranges()[range]);
    }
    assert_eq!(gpu, plans.scan_ranges());
}

#[test]
fn vacant_stable_path_slot_does_not_clobber_path_zero_scan_range() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE, 1.0);
    for x in [0.0, 32.0] {
        canvas.push_path(
            Rect::new(x, 0.0, x + 32.0, 16.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    let mut plans = PersistentPathPlans::default();
    plans.update(&canvas, None);
    let first = plans.scan_ranges()[0];

    canvas.path_records[1] = Default::default();
    plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));

    assert_eq!(plans.scan_ranges()[0], first);
    assert_eq!(plans.scan_ranges()[1], GpuScanChunkRange::default());
}

#[test]
fn cumsum_plan_splits_each_backdrop_row_into_fixed_size_chunks() {
    let row_tiles = CUMSUM_CHUNK_SIZE + 17;
    let mut canvas = Canvas::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2, 1.0);
    canvas.push_path(
        Rect::new(
            0.0,
            0.0,
            f64::from(row_tiles * crate::TILE_SIZE),
            f64::from(crate::TILE_SIZE * 2),
        )
        .to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let lengths = GpuBufferLengths::from_scene(&canvas);
    let plan = build_cumsum_plan(&canvas);

    assert_eq!(lengths.cumsum_row_count, 2);
    assert_eq!(lengths.cumsum_chunk_count, 4);
    assert_eq!(plan.row_chunk_starts, vec![0, 2]);
    assert_eq!(plan.row_chunk_ends, vec![2, 4]);
    assert_eq!(
        plan.chunk_backdrop_offsets,
        vec![
            0,
            CUMSUM_CHUNK_SIZE,
            row_tiles,
            row_tiles + CUMSUM_CHUNK_SIZE
        ]
    );
    assert_eq!(
        plan.chunk_lens,
        vec![CUMSUM_CHUNK_SIZE, 17, CUMSUM_CHUNK_SIZE, 17]
    );
}
