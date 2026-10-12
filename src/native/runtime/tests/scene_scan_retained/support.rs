//! Helpers compare complete geometry independently of atomic edge emission order.
use super::*;

pub(super) fn leaf() -> Canvas {
    let mut canvas = Canvas::new(96, 96, 1.0);
    let mut path = BezPath::new();
    path.move_to((3.0, 5.0));
    path.curve_to((80.0, 1.0), (2.0, 80.0), (70.0, 85.0));
    path.line_to((3.0, 5.0));
    canvas.push_path(
        path,
        Color::WHITE,
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.1,
    );
    canvas
}

pub(super) fn retained() -> RetainedScene {
    let root = RetainedNodeId::for_owner(900);
    let mut scene = RetainedScene::new(96, 96, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(901),
            Rc::new(leaf()),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    scene
}

pub(super) fn geometry_passes(batch: &ComputeBatch) -> usize {
    batch
        .passes()
        .iter()
        .filter(|pass| {
            pass.shader.entry.starts_with("scan_") || pass.shader.entry.starts_with("cumsum_")
        })
        .count()
}

pub(super) fn config(batch: &ComputeBatch) -> Option<Vec<u32>> {
    use crate::native::runtime::compute::Resource;
    let pass = batch
        .passes()
        .iter()
        .find(|pass| pass.shader.entry == "scan_clear")?;
    let id = pass
        .bindings
        .iter()
        .find(|(binding, _)| binding.slot == 0)?
        .1;
    let Resource::Buffer(bytes) = &batch.resources()[id.index()] else {
        panic!("scan config");
    };
    Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect(),
    )
}

pub(super) fn update(materializer: &mut PersistentSceneMaterializer, scene: &RetainedScene) {
    materializer.update(scene, scene.changes_since(materializer.version()));
}

// Segment atomics may emit edges in a different order. Compare each live tile's
// backdrop and complete edge multiset instead of accidentally asserting scheduling.
pub(super) type Geometry = Vec<(u32, Vec<[u32; 5]>)>;
pub(super) fn geometry(
    context: &crate::NativeContext,
    cache: &mut SceneCache,
    canvas: &Canvas,
) -> Result<(Geometry, Option<Vec<u32>>)> {
    let mut batch = gpu_support::batch(context);
    let scene = cache.record(&mut batch, canvas, None, None, 65535)?;
    let config = config(&batch);
    for id in [
        scene.scan.backdrops,
        scene.scan.tile_segment_ranges,
        scene.scan.segments,
    ] {
        batch.readback(id)?;
    }
    let output = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?
        .readback()?;
    let words: Vec<Vec<u32>> = output
        .iter()
        .map(|bytes| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes))
                .collect()
        })
        .collect();
    let mut geometry = Vec::new();
    for (id, path) in canvas
        .path_records
        .iter()
        .enumerate()
        .filter(|(id, path)| path.is_live_at(*id))
    {
        for tile in path.data_offset..path.data_offset + path.data_len {
            let start = words[1][tile as usize * 2] as usize;
            let end = words[1][tile as usize * 2 + 1] as usize;
            let mut edges: Vec<[u32; 5]> = words[2][start * 5..end * 5].as_chunks::<5>().0.to_vec();
            edges.sort_unstable();
            geometry.push((words[0][tile as usize], edges));
        }
        assert_eq!(path.path_id as usize, id);
    }
    Ok((geometry, config))
}

pub(super) fn paint() -> Canvas {
    let mut canvas = Canvas::new(96, 96, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(1.0, 1.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas
}
