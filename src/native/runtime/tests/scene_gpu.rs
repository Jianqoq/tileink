use super::*;

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn scene_work_allocations_are_persistent_and_reused() -> Result<()> {
    use crate::native::runtime::compute::{Resource, SurfacePool};
    use crate::native::{NativeBackend, NativeContext, NativeContextOptions};
    use std::{cell::RefCell, rc::Rc};
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: false,
        },
    )?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut canvas = Canvas::new(64, 32, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(1.0, 2.0, 45.0, 26.0),
        crate::Radius::ZERO,
        peniko::Color::from_rgb8(210, 40, 60),
    );
    let mut cache = SceneCache::default();
    let mut previous = Vec::new();
    let mut contents = None;
    for frame in 0..3 {
        let mut batch = ComputeBatch::with_surfaces(pool.clone());
        let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
        let ids = [
            scene.work,
            scene.chunks,
            scene.spills,
            scene.scan.backdrops,
            scene.scan.tile_segment_ranges,
            scene.scan.segments,
        ];
        let allocations: Vec<_> = ids
            .iter()
            .map(|id| {
                let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
                    panic!("scene work storage must survive frame retirement");
                };
                upload.buffer.state.clone()
            })
            .collect();
        if frame > 0 {
            assert!(
                allocations
                    .iter()
                    .zip(&previous)
                    .all(|(a, b)| Rc::ptr_eq(a, b))
            );
        }
        if frame > 0 {
            for id in [
                scene.chunks,
                scene.spills,
                scene.scan.backdrops,
                scene.scan.tile_segment_ranges,
                scene.scan.segments,
            ] {
                let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
                    unreachable!()
                };
                assert!(
                    upload.bytes.is_empty(),
                    "GPU-initialized work must not be zero-uploaded every frame"
                );
            }
            let Resource::PersistentBuffer(upload) = &batch.resources()[scene.work.index()] else {
                unreachable!()
            };
            assert!(
                upload.bytes.is_empty(),
                "unchanged tile bins must not be reuploaded"
            );
        }
        previous = allocations;
        for id in ids {
            batch.readback(id)?;
        }
        let result = context.adapter.submit_compute(&batch).unwrap().readback()?;
        if let Some(expected) = &contents {
            assert_eq!(&result, expected);
        }
        contents = Some(result);
    }
    canvas.push_rect(
        peniko::kurbo::Rect::new(12.0, 4.0, 63.0, 30.0),
        crate::Radius::ZERO,
        peniko::Color::from_rgb8(40, 80, 220),
    );
    let mut batch = ComputeBatch::with_surfaces(pool);
    let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
    let Resource::PersistentBuffer(upload) = &batch.resources()[scene.work.index()] else {
        unreachable!()
    };
    assert!(
        !upload.bytes.is_empty(),
        "changed tile bins must be uploaded"
    );
    batch.readback(scene.work)?;
    let changed = context.adapter.submit_compute(&batch).unwrap().readback()?;
    assert_ne!(&changed[0], &contents.unwrap()[0]);
    Ok(())
}
