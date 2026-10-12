use crate::native::runtime::renderer::FrameOptions;
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, Resource, SurfacePool},
    renderer::images::Images,
};
use crate::{NativeContext, NativeContextOptions};
use std::{cell::RefCell, rc::Rc};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_image_retention_pins_gpu_output_without_copying_or_pool_reuse() -> Result<()> {
    let context = crate::native::runtime::gpu_support::context()?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut batch = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
    let color = [10, 20, 30, 255];
    let source = batch.reusable_surface([5, 3], u32::from_le_bytes(color))?;
    let original = batch.persistent_texture(source)?.unwrap().clone();
    let commands = batch.commands().len();
    let retained = batch.retain_image(source)?;
    assert!(Rc::ptr_eq(&original.state, &retained.state));
    assert_eq!(commands, batch.commands().len());
    let again = batch.retain_image(source)?;
    assert!(Rc::ptr_eq(&again.state, &retained.state));
    assert_eq!(batch.import_texture(&retained)?, source);
    batch.readback(source)?;
    let first = context
        .adapter
        .submit_compute(&batch)
        .map_err(|error| format!("{error:?}"))?;
    drop(batch);
    drop(original);
    drop(again);

    // A cached image pins its lease while later batches acquire and write other storage.
    let mut next = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
    let other = next.reusable_surface([5, 3], u32::from_le_bytes([80, 90, 100, 255]))?;
    assert!(!Rc::ptr_eq(
        &retained.state,
        &next.persistent_texture(other)?.unwrap().state
    ));
    next.readback(other)?;
    let second = context
        .adapter
        .submit_compute(&next)
        .map_err(|error| format!("{error:?}"))?;
    drop(next);
    drop(pool);
    assert_eq!(second.readback()?, vec![[80, 90, 100, 255].repeat(15)]);
    assert_eq!(first.readback()?, vec![color.repeat(15)]);
    assert_eq!(
        retained.readback()?.readback()?.pixels,
        vec![u32::from_le_bytes(color); 15]
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn abandoned_vector_cache_is_rebuilt_before_sampling() -> Result<()> {
    use crate::{Canvas, ImageKey, shared::image_resource::ImageResourceStore};
    let context = crate::native::runtime::gpu_support::context()?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut resources = ImageResourceStore::default();
    resources.insert(ImageKey(1), Rc::new(Canvas::new(2050, 1, 1.0)));
    let empty = ImageResourceStore::default();
    let table_len = crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY;
    let signature = resources.upload_signature(&empty, 4096, 4, table_len);
    let upload = resources.upload_merged(&empty, 4096, 4, table_len, None);
    let mut cache = None;
    let mut renders = 0;
    let mut previous = None;
    for (frame, color) in [[10, 20, 30, 255], [80, 90, 100, 255]]
        .into_iter()
        .enumerate()
    {
        let mut batch = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
        let images =
            Images::record_cached(&mut batch, &upload, &mut cache, signature, |batch, _| {
                renders += 1;
                batch.reusable_surface([2050, 1], u32::from_le_bytes(color))
            })?;
        let Resource::TextureTable(table) = &batch.resources()[images.textures().table.index()]
        else {
            panic!()
        };
        let source = table[0];
        let texture = batch.persistent_texture(source)?.unwrap().clone();
        if frame == 0 {
            previous = Some(texture);
            // Drop the batch without publishing either its native pixels or cache acceptance.
        } else {
            let previous = previous.as_ref().unwrap();
            assert!(!previous.state.initialized.get());
            assert!(!Rc::ptr_eq(&previous.state, &texture.state));
            batch.readback(source)?;
            let receipt = context
                .adapter
                .submit_compute(&batch)
                .map_err(|error| format!("{error:?}"))?;
            assert_eq!(receipt.readback()?, vec![color.repeat(2050)]);
        }
    }
    assert_eq!(renders, 2);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_image_cache_reuses_initialized_arrays_without_upload() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: The serial GPU fixture enables validation before creating its context; the creation gate prevents late debug-layer changes.
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
    let mut first = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
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
    let backend = super::backend();
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    for width in [4, 2050] {
        let mut child = Canvas::new(width, 4, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, f64::from(width), 4.0),
            Radius::ZERO,
            Color::from_rgb8(71, 19, 23),
        );
        let child = Rc::new(child);
        let mut canvas = Canvas::new(width * 2, 4, 1.0);
        for (key, x) in [(ImageKey(1), 0.0), (ImageKey(2), f64::from(width))] {
            canvas
                .push_image_key(
                    Rect::new(x, 0.0, x + f64::from(width), 4.0),
                    key,
                    Extend::Pad,
                    PatternSampling::Nearest,
                )
                .unwrap();
        }
        let mut resources = crate::shared::image_resource::ImageResourceStore::default();
        resources.insert(ImageKey(1), std::rc::Rc::clone(&child));
        resources.insert(ImageKey(2), std::rc::Rc::clone(&child));
        let mut recording = super::super::renderer::recording::Recording::default();
        let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
        for _ in 0..2 {
            let mut batch = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
            let target = recording.record(
                &mut batch,
                &canvas,
                &resources,
                None,
                super::super::renderer::recording::Limits {
                    image_dimension: 4096,
                    atlas_pages: 4,
                    texture_table_len: crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
                    dispatch_dimension: 65535,
                },
                FrameOptions::default(),
            )?;
            if width == 2050 {
                let table = batch
                    .resources()
                    .iter()
                    .rev()
                    .find_map(|resource| match resource {
                        Resource::TextureTable(table) => Some(table),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    table[0], table[1],
                    "alias keys must bind the same child image"
                );
            }
            batch.readback(target)?;
            let receipt = context
                .adapter
                .submit_compute(&batch)
                .map_err(|error| format!("{error:?}"))?;
            assert_eq!(
                receipt.readback()?,
                vec![[71, 19, 23, 255].repeat(width as usize * 8)]
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[cfg(any(feature = "dx12", feature = "vulkan"))]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_image_uploads_preserve_queued_array_versions() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: The serial GPU fixture enables validation before creating its context; the creation gate prevents late debug-layer changes.
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
    let mut abandoned = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
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
        let mut batch = ComputeBatch::with_surfaces(std::rc::Rc::clone(&pool));
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
            let repeated = batch.retain_image(id)?;
            assert!(Rc::ptr_eq(&retained.state, &repeated.state));
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
