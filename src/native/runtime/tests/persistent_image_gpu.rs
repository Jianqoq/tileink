use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, Resource, SurfacePool},
    renderer::images::Images,
};
use crate::{NativeContext, NativeContextOptions};
use std::{cell::RefCell, rc::Rc};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_image_cache_reuses_initialized_arrays_without_upload() -> Result<()> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = super::backend();
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let resources = crate::shared::image_resource::ImageResourceStore::default();
    let signature = resources.upload_signature(&resources, 32, 4, 0);
    let upload = resources.upload_merged(&resources, 32, 4, 0, None);
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut cache = None;
    let mut first = ComputeBatch::with_surfaces(pool.clone());
    let images = Images::record_cached(&mut first, &upload, &mut cache, signature, |_, _| {
        panic!("empty vector resources")
    })?;
    #[cfg(any(feature = "dx12", feature = "vulkan"))]
    assert!(
        first.commands().is_empty(),
        "CPU image uploads must initialize cache storage directly, without a second texture copy"
    );
    let atlas = images.textures().atlas;
    let Resource::Texture(first_atlas) = &first.resources()[atlas.index()] else {
        unreachable!()
    };
    let identity = Rc::downgrade(&first_atlas.persistent.as_ref().unwrap().state);
    first.readback(atlas)?;
    let receipt = context
        .adapter
        .submit_compute(&first)
        .map_err(|error| format!("{error:?}"))?;
    drop(first);
    let mut next = ComputeBatch::with_surfaces(pool);
    let images = Images::record_cached(&mut next, &upload, &mut cache, signature, |_, _| {
        panic!("cached vectors must not render")
    })?;
    let atlas = images.textures().atlas;
    let Resource::Texture(next_atlas) = &next.resources()[atlas.index()] else {
        unreachable!()
    };
    assert!(std::rc::Weak::ptr_eq(
        &identity,
        &Rc::downgrade(&next_atlas.persistent.as_ref().unwrap().state)
    ));
    assert!(
        next.resources()
            .iter()
            .all(|resource| resource.bytes().is_empty())
    );
    next.readback(atlas)?;
    let second = context
        .adapter
        .submit_compute(&next)
        .map_err(|error| format!("{error:?}"))?;
    drop(next);
    drop(cache);
    assert_eq!(second.readback()?, vec![vec![0; 4]]);
    assert_eq!(receipt.readback()?, vec![vec![0; 4]]);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_vector_aliases_share_one_recording_per_batch() -> Result<()> {
    // Also regresses AMD Vulkan pattern reads being discarded by a miscompiled
    // brush bounds guard: the child image uploads correctly but the parent is empty.
    use crate::{Canvas, ImageKey, PatternSampling, Radius};
    use peniko::{Color, Extend, kurbo::Rect};
    let mut child = Canvas::new(4, 4, 1.0);
    child.push_rect(
        Rect::new(0.0, 0.0, 4.0, 4.0),
        Radius::ZERO,
        Color::from_rgb8(71, 19, 23),
    );
    let child = Rc::new(child);
    let mut canvas = Canvas::new(8, 4, 1.0);
    for (key, x) in [(ImageKey(1), 0.0), (ImageKey(2), 4.0)] {
        canvas
            .push_image_key(
                Rect::new(x, 0.0, x + 4.0, 4.0),
                key,
                Extend::Pad,
                PatternSampling::Nearest,
            )
            .unwrap();
    }
    let backend = super::backend();
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let mut resources = crate::shared::image_resource::ImageResourceStore::default();
    resources.insert(ImageKey(1), child.clone());
    resources.insert(ImageKey(2), child.clone());
    let mut recording = super::super::renderer::recording::Recording::default();
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    for _ in 0..2 {
        let mut batch = ComputeBatch::with_surfaces(pool.clone());
        let target = recording.record(
            &mut batch,
            &canvas,
            &resources,
            None,
            super::super::renderer::recording::Limits {
                image_dimension: 64,
                atlas_pages: 4,
                texture_table_len: 0,
                dispatch_dimension: 65535,
            },
            Default::default(),
        )?;
        batch.readback(target)?;
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(receipt.readback()?, vec![[71, 19, 23, 255].repeat(32)]);
    }
    context.check_validation()?;
    Ok(())
}

#[cfg(any(feature = "dx12", feature = "vulkan"))]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_image_uploads_preserve_queued_array_versions() -> Result<()> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut receipts = Vec::new();
    let mut pinned = Vec::new();
    let mut abandoned = ComputeBatch::with_surfaces(pool.clone());
    let image = abandoned.texture_array_rgba8([2, 2, 2], vec![42; 32])?;
    let unpublished = abandoned.retain_image(image)?;
    drop(abandoned);
    assert!(!unpublished.state.initialized.get());
    assert_eq!(unpublished.state.content_version.get(), 0);
    assert!(ComputeBatch::new().import_texture(&unpublished).is_err());
    drop(unpublished);
    for revision in 0..3 {
        if revision == 2 {
            pinned.clear();
        }
        let mut batch = ComputeBatch::with_surfaces(pool.clone());
        let mut expected = Vec::new();
        for (index, (size, layers, array)) in
            [([3, 2], 1, false), ([5, 3], 2, true), ([3, 2], 1, true)]
                .into_iter()
                .enumerate()
        {
            let bytes: Vec<u8> = (0..size[0] * size[1] * layers)
                .flat_map(|pixel| [revision as u8 + 1, index as u8 + 17, pixel as u8, 255])
                .collect();
            let id = if array {
                batch.texture_array_rgba8([size[0], size[1], layers], bytes.clone())?
            } else {
                batch.texture_rgba8(size, bytes.clone())?
            };
            let retained = batch.retain_image(id)?;
            assert_eq!(batch.import_texture(&retained)?, id);
            assert!(
                batch.retain_image(id).is_err(),
                "retention must not alias an already published image"
            );
            if revision == 0 {
                pinned.push(retained);
            }
            batch.readback(id)?;
            expected.push(bytes);
        }
        assert!(
            batch.commands().is_empty(),
            "initial bytes need no snapshot or clear dispatch"
        );
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        if revision == 0 {
            assert!(pinned.iter().all(
                |image| image.state.initialized.get() && image.state.content_version.get() == 1
            ));
        }
        drop(batch);
        receipts.push((receipt, expected));
    }
    drop(pool);
    // Read newest first after releasing all public image owners: queue ordering,
    // rather than a CPU wait, must preserve each upload's complete array contents.
    for (receipt, expected) in receipts.into_iter().rev() {
        assert_eq!(receipt.readback()?, expected);
    }
    // Completed uploads become staging-cache candidates. Reusing a larger mapping
    // for smaller array rows must overwrite every texel without copying row padding.
    for width in [65, 3, 65] {
        let bytes: Vec<u8> = (0..width * 3 * 2 * 4)
            .map(|i| (i * 17 + width) as u8)
            .collect();
        let mut batch = ComputeBatch::new();
        let id = batch.texture_array_rgba8([width, 3, 2], bytes.clone())?;
        batch.readback(id)?;
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(receipt.readback()?, vec![bytes]);
    }
    context.check_validation()?;
    Ok(())
}
