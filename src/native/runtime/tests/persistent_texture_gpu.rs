use super::super::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;
use crate::{NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_surface_pool_reuses_only_resolved_batches_and_clears_old_pixels() -> Result<()> {
    use crate::native::runtime::compute::{Resource, SurfacePool};
    use std::{cell::RefCell, rc::Rc};
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
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut first = ComputeBatch::with_surfaces(pool.clone());
    let a = first.reusable_surface([3, 2], 0xff231347)?.unwrap();
    let b = first.reusable_surface([3, 2], 0xff1d710b)?.unwrap();
    let allocation = |batch: &ComputeBatch, id: super::super::compute::ResourceId| {
        let Resource::Texture(texture) = &batch.resources()[id.index()] else {
            panic!("surface texture")
        };
        Rc::downgrade(&texture.persistent.as_ref().unwrap().state)
    };
    let original = [allocation(&first, a), allocation(&first, b)];
    assert!(
        !std::rc::Weak::ptr_eq(&original[0], &original[1]),
        "siblings must not alias"
    );
    first.readback(a)?;
    first.readback(b)?;
    let first_receipt = context
        .adapter
        .submit_compute(&first)
        .map_err(|e| format!("{e:?}"))?;
    drop(first);
    let mut second = ComputeBatch::with_surfaces(pool.clone());
    for _ in 0..2 {
        let image = second.reusable_surface([3, 2], 0)?.unwrap();
        let reused = allocation(&second, image);
        assert!(
            original
                .iter()
                .any(|old| std::rc::Weak::ptr_eq(old, &reused)),
            "must reuse GPU storage"
        );
        second.readback(image)?;
    }
    let second_receipt = context
        .adapter
        .submit_compute(&second)
        .map_err(|e| format!("{e:?}"))?;
    drop(second);
    assert_eq!(second_receipt.readback()?, vec![vec![0; 24]; 2]);
    let expected = [0xff231347u32, 0xff1d710b].map(|color| color.to_le_bytes().repeat(6));
    assert_eq!(first_receipt.readback()?, expected);
    let mut abandoned = ComputeBatch::with_surfaces(pool.clone());
    let resized = abandoned.reusable_surface([9, 4], 0xff123456)?.unwrap();
    let unsubmitted = allocation(&abandoned, resized);
    // A later recording error discards all commands, including initialization.
    // The next resolved boundary may reuse storage, but cannot publish pixels
    // or initialization from this abandoned batch.
    assert!(
        abandoned
            .copy_texture(super::super::compute::TextureCopy {
                source: resized,
                destination: resized,
                source_origin: [0; 3],
                destination_origin: [0; 3],
                extent: [9, 4, 1],
            })
            .is_err()
    );
    drop(abandoned);
    assert!(!unsubmitted.upgrade().unwrap().initialized.get());
    let mut retry = ComputeBatch::with_surfaces(pool.clone());
    let resized = retry.reusable_surface([9, 4], 0)?.unwrap();
    assert!(std::rc::Weak::ptr_eq(
        &unsubmitted,
        &allocation(&retry, resized)
    ));
    retry.readback(resized)?;
    let retried = context
        .adapter
        .submit_compute(&retry)
        .map_err(|e| format!("{e:?}"))?;
    drop(retry);
    drop(pool);
    assert_eq!(retried.readback()?, vec![vec![0; 9 * 4 * 4]]);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_persistent_texture_preserves_untouched_pixels_across_submissions() -> Result<()> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = super::backend();
    let options = NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: true,
    };
    let context = NativeContext::new(backend, &options)?;
    let texture = context.create_texture(7, 3)?;
    let mut first = ComputeBatch::new();
    let image = first.import_texture(&texture)?;
    assert_eq!(image, first.import_texture(&texture.clone())?);
    filter::encode(
        &mut first,
        BasicFilter::Clear,
        FilterConfig {
            width: 7,
            height: 3,
            region_width: 3,
            region_height: 2,
            clear_color: u32::from_le_bytes([71, 19, 23, 255]),
            ..Default::default()
        },
        None,
        None,
        image,
    )?;
    let mut second = ComputeBatch::new();
    let image = second.import_texture(&texture)?;
    filter::encode(
        &mut second,
        BasicFilter::Clear,
        FilterConfig {
            width: 7,
            height: 3,
            region_x0: 2,
            region_y0: 1,
            region_width: 2,
            region_height: 1,
            clear_color: u32::from_le_bytes([11, 113, 29, 255]),
            ..Default::default()
        },
        None,
        None,
        image,
    )?;
    second.readback(image)?;
    let other = NativeContext::new(backend, &options)?;
    assert!(other.adapter.submit_compute(&second).is_err());
    let first_receipt = context
        .adapter
        .submit_compute(&first)
        .map_err(|e| format!("{e:?}"))?;
    let second_receipt = context
        .adapter
        .submit_compute(&second)
        .map_err(|e| format!("{e:?}"))?;
    let mut third = ComputeBatch::new();
    let third_image = third.import_texture(&texture)?;
    assert!(
        third.passes().is_empty(),
        "initialized read-only imports must not upload clear uniforms"
    );
    third.readback(third_image)?;
    let third_receipt = context
        .adapter
        .submit_compute(&third)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        texture.state.content_version.get(),
        2,
        "readback must not publish a write"
    );
    drop(third);
    drop(first);
    drop(second);
    drop(texture);
    let third_output = third_receipt.readback()?;
    let output = second_receipt.readback()?;
    assert_eq!(third_output, output);
    for y in 0..3 {
        for x in 0..7 {
            let expected = if y == 1 && (2..4).contains(&x) {
                [11, 113, 29, 255]
            } else if x < 3 && y < 2 {
                [71, 19, 23, 255]
            } else {
                [0, 0, 0, 0]
            };
            assert_eq!(&output[0][(y * 7 + x) * 4..(y * 7 + x + 1) * 4], expected);
        }
    }
    first_receipt.readback()?;
    // Exercise COPY_SOURCE/COPY_DEST followed by UAV writes in later submissions.
    // Both allocations must return to the shared queue's persistent state.
    let source = context.create_texture(7, 3)?;
    let destination = context.create_texture(7, 3)?;
    let mut copy = ComputeBatch::new();
    let uploaded = copy.texture_rgba8([7, 3], output[0].clone())?;
    let source_id = copy.import_texture(&source)?;
    let destination_id = copy.import_texture(&destination)?;
    for (source, destination) in [(uploaded, source_id), (source_id, destination_id)] {
        copy.copy_texture(super::super::compute::TextureCopy {
            source,
            destination,
            source_origin: [0; 3],
            destination_origin: [0; 3],
            extent: [7, 3, 1],
        })?;
    }
    let copied = context.submit_compute(&copy)?;
    let mut edit = ComputeBatch::new();
    for texture in [&source, &destination] {
        let image = edit.import_texture(texture)?;
        filter::encode(
            &mut edit,
            BasicFilter::Clear,
            FilterConfig {
                width: 7,
                height: 3,
                region_x0: 6,
                region_y0: 2,
                region_width: 1,
                region_height: 1,
                clear_color: u32::from_le_bytes([13, 17, 23, 255]),
                ..Default::default()
            },
            None,
            None,
            image,
        )?;
    }
    let edited = context.submit_compute(&edit)?;
    let mut expected = output[0].clone();
    expected[80..84].copy_from_slice(&[13, 17, 23, 255]);
    for texture in [&source, &destination] {
        let image = texture.readback()?.readback()?;
        assert_eq!(bytemuck::cast_slice::<_, u8>(&image.pixels), expected);
    }
    edited.wait()?;
    copied.wait()?;
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_surface_pool_preserves_pinned_history_across_batches() -> Result<()> {
    use crate::native::runtime::compute::SurfacePool;
    use std::{cell::RefCell, rc::Rc};
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
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut first = ComputeBatch::with_surfaces(pool.clone());
    let image = first.reusable_surface([3, 2], 0xff231347)?.unwrap();
    let history = first.persistent_texture(image)?.unwrap().clone();
    let submitted = context.submit_compute(&first)?;
    drop(first);
    let mut next = ComputeBatch::with_surfaces(pool);
    let scratch = next.reusable_surface([3, 2], 0)?.unwrap();
    assert!(!Rc::ptr_eq(
        &history.state,
        &next.persistent_texture(scratch)?.unwrap().state
    ));
    let imported = next.import_texture(&history)?;
    next.readback(imported)?;
    next.readback(scratch)?;
    let result = context
        .adapter
        .submit_compute(&next)
        .map_err(|error| format!("{error:?}"))?;
    drop(next);
    drop(history);
    assert_eq!(
        result.readback()?,
        vec![0xff231347u32.to_le_bytes().repeat(6), vec![0; 24]]
    );
    submitted.wait()?;
    context.check_validation()?;
    Ok(())
}
