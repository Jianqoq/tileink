use super::*;
use crate::native::runtime::compute::SurfacePool;
use crate::native::{NativeBackend, NativeContext, NativeContextOptions};
use std::cell::RefCell;

#[test]
fn cached_buffers_require_native_storage_without_substituting_cpu_uploads() {
    let mut cache = CachedBuffer::default();
    let mut batch = ComputeBatch::new();
    assert!(cache.upload(&mut batch, &[7u32], None).is_err());
    assert!(
        cache
            .patches_delta(&mut batch, 16, &[(4, &[1, 2, 3, 4])], &[])
            .is_err()
    );
    assert!(cache.scratch(&mut batch, 1, 4).is_err());
    assert!(batch.resources().is_empty());
    assert!(batch.commands().is_empty());
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn patch_snapshots_reinitialize_new_grown_and_abandoned_storage() -> Result<()> {
    let context = crate::native::runtime::gpu_support::context()?;
    let surfaces = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut cache = CachedBuffer::default();
    for (size, offset, value, submit, dirty) in [
        (16, 4, 1u32, true, false),
        (16, 4, 2, true, true),
        (16, 4, 3, false, true),
        (16, 4, 4, true, false),
        (32, 20, 5, true, false),
        (32, 20, 6, true, true),
        (32, 20, 6, true, false),
    ] {
        let mut batch = ComputeBatch::with_surfaces(surfaces.clone());
        let bytes = value.to_le_bytes();
        let full = [(offset, bytes.as_slice())];
        let id = cache.patches_delta(&mut batch, size, &full, if dirty { &full } else { &[] })?;
        if submit {
            batch.readback(id)?;
            let output = context
                .adapter
                .submit_compute(&batch)
                .map_err(|error| format!("{error:?}"))?
                .readback()?;
            let mut expected = vec![0; size];
            expected[offset..offset + 4].copy_from_slice(&bytes);
            assert_eq!(output, vec![expected]);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn unchanged_immediate_data_avoids_upload_but_later_edits_are_visible() {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU").unwrap()),
            validation: false,
        },
    )
    .unwrap();
    let surfaces = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut cache = CachedBuffer::default();
    for (frame, (len, value, submit)) in [
        (4096, 7u32, true),
        (4096, 7, true),
        (4096, 9, true),
        (4096, 9, false),
        (4096, 9, true),
        (2048, 9, true),
    ]
    .into_iter()
    .enumerate()
    {
        let mut batch = ComputeBatch::with_surfaces(Rc::clone(&surfaces));
        let id = cache.upload(&mut batch, &vec![value; len], None).unwrap();
        let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
            panic!("expected persistent upload");
        };
        assert_eq!(
            upload.bytes.is_empty(),
            frame == 1 || frame == 3,
            "frame {frame}"
        );
        if !upload.bytes.is_empty() {
            assert_eq!(&upload.bytes[..4], &value.to_le_bytes());
        }
        if submit {
            context.submit_compute(&batch).unwrap().wait().unwrap();
        }
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn dense_dirty_ranges_use_one_copy_and_keep_later_sparse_edits() {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU").unwrap()),
            validation: false,
        },
    )
    .unwrap();
    let surfaces = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut cache = CachedBuffer::default();
    let mut data = vec![0u32; 2048];
    for (frame, dirty) in [
        None,
        Some((0..2048).step_by(2).map(|i| i..i + 1).collect::<Vec<_>>()),
        Some(std::iter::once(3..4).collect()),
    ]
    .into_iter()
    .enumerate()
    {
        if frame == 1 {
            for value in data.iter_mut().step_by(2) {
                *value = 1;
            }
        } else if frame == 2 {
            data[3] = 7;
        }
        let mut batch = ComputeBatch::with_surfaces(Rc::clone(&surfaces));
        let id = cache.upload(&mut batch, &data, dirty.as_deref()).unwrap();
        let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
            panic!("expected persistent upload")
        };
        assert!(upload.copies.len() <= 1, "frame {frame}");
        assert!(
            batch.passes().is_empty(),
            "frame {frame} should not need a scatter dispatch"
        );
        batch.readback(id).unwrap();
        let bytes = context
            .adapter
            .submit_compute(&batch)
            .unwrap()
            .readback()
            .unwrap();
        assert_eq!(
            &bytes[0][..data.len() * 4],
            bytemuck::cast_slice::<u32, u8>(&data)
        );
    }
}
