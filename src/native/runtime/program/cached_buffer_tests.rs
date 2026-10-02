use super::*;
use crate::native::runtime::compute::SurfacePool;
use crate::native::{NativeBackend, NativeContext, NativeContextOptions};
use std::cell::RefCell;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn unchanged_immediate_data_avoids_upload_but_later_edits_are_visible() {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
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
