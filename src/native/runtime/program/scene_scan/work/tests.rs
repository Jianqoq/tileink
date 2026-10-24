use super::*;
use crate::{
    RetainedNodeId, RetainedParent, RetainedScene, canvas::SceneBufferChanges,
    retained_scene::PersistentSceneMaterializer, shared::gpu_plan::PersistentPathPlans,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

fn canvas() -> Canvas {
    let root = RetainedNodeId::for_owner(800);
    let mut source = Canvas::new(96, 96, 1.0);
    for x in [1.0, 35.0, 67.0] {
        source.push_path(
            Rect::new(x, 2.0, x + 25.0, 80.0).to_path(0.1),
            Color::WHITE,
            Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.1,
        );
    }
    let mut scene = RetainedScene::new(96, 96, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(801),
            Rc::new(source),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    (*PersistentSceneMaterializer::new(&scene).canvas()).clone()
}

fn next(canvas: &Canvas) -> Canvas {
    let mut next = canvas.clone();
    let version = next.persistent_frame.as_ref().unwrap().version;
    next.persistent_frame.as_mut().unwrap().version += 1;
    next.persistent_frame.as_mut().unwrap().delta = None;
    next.buffer_changes = Some(SceneBufferChanges {
        from_version: Some(version),
        ..Default::default()
    });
    next
}

#[test]
fn line_dirty_ranges_close_over_all_edges_chunks_and_rows_of_their_owner() {
    let old = canvas();
    let history = History::new(&old, Rc::new(Cell::new(true)));
    let mut current = next(&old);
    let path = current.path_records[1];
    current.buffer_changes.as_mut().unwrap().lines =
        std::iter::once(path.line_start as usize + 1..path.line_start as usize + 2).collect();
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&current, &mut plans);
    let work = ScanWork::new(
        &prepared,
        Some(&history),
        true,
        &mut DenseIndexSet::default(),
    )
    .unwrap();
    assert!(!work.reuse);
    assert_eq!(work.config[6], 1);
    assert_eq!(
        &work.indices[..work.config[8] as usize],
        &(path.line_start..path.line_start + path.line_count).collect::<Vec<_>>()
    );
    assert_eq!(
        &work.indices[work.config[8] as usize..work.config[9] as usize],
        [1]
    );
    let chunks = prepared.plans.scan_ranges()[1];
    assert_eq!(
        &work.indices[work.config[9] as usize..work.config[10] as usize],
        &(chunks.start..chunks.end).collect::<Vec<_>>()
    );
    assert_eq!(
        &work.indices[work.config[10] as usize..],
        &(path.data_offset..path.data_offset + path.data_len).collect::<Vec<_>>()
    );
    assert_eq!(
        work.cumsum.as_ref().unwrap().chunk_lens.iter().sum::<u32>(),
        path.data_len
    );
}

#[test]
fn missing_storage_unaccepted_versions_gaps_and_foreign_materializations_require_full_scan() {
    let old = canvas();
    let mut current = next(&old);
    let accepted = Rc::new(Cell::new(true));
    let history = History::new(&old, Rc::clone(&accepted));
    let mut plans = PersistentPathPlans::default();
    let classify = |canvas: &Canvas, preserve, plans: &mut PersistentPathPlans| {
        let prepared = PreparedScan::new(canvas, plans);
        ScanWork::new(
            &prepared,
            Some(&history),
            preserve,
            &mut DenseIndexSet::default(),
        )
        .unwrap()
    };
    assert!(classify(&current, true, &mut plans).reuse);
    assert!(!classify(&current, false, &mut plans).reuse);
    accepted.set(false);
    assert!(!classify(&current, true, &mut plans).reuse);
    accepted.set(true);
    current.buffer_changes.as_mut().unwrap().from_version = None;
    assert!(!classify(&current, true, &mut plans).reuse);
    current.buffer_changes.as_mut().unwrap().from_version =
        Some(old.persistent_frame.as_ref().unwrap().version);
    current.materialization_key = canvas().materialization_key;
    assert!(!classify(&current, true, &mut plans).reuse);
    current.materialization_key = old.materialization_key;
    current.buffer_changes.as_mut().unwrap().full_scene_sync = true;
    assert!(!classify(&current, true, &mut plans).reuse);
}

#[test]
fn removed_paths_are_not_dispatched_and_dense_updates_keep_direct_indexing() {
    let old = canvas();
    let history = History::new(&old, Rc::new(Cell::new(true)));
    let mut current = next(&old);
    current.path_records[1] = crate::shared::path::PathRecord::default();
    current.buffer_changes.as_mut().unwrap().paths = std::iter::once(1..2).collect();
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&current, &mut plans);
    let work = ScanWork::new(
        &prepared,
        Some(&history),
        true,
        &mut DenseIndexSet::default(),
    )
    .unwrap();
    assert!(work.reuse);
    current = next(&old);
    // Dense updates must use direct dispatch even with vacant physical slots.
    assert!(current.path_records.len() > 3);
    current.buffer_changes.as_mut().unwrap().paths =
        std::iter::once(0..current.path_records.len()).collect();
    let prepared = PreparedScan::new(&current, &mut plans);
    let work = ScanWork::new(
        &prepared,
        Some(&history),
        true,
        &mut DenseIndexSet::default(),
    )
    .unwrap();
    assert!(!work.reuse);
    assert_eq!(work.config[6], 0);
    assert!(work.indices.is_empty());
}
