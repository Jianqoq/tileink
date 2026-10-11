use super::*;
use crate::native::runtime::{compute::Resource, gpu_support};

#[test]
fn surface_allocation_requires_a_renderer_pool_without_recording_cpu_pixels() {
    let mut batch = ComputeBatch::new();
    assert!(Surface::allocate(&mut batch, [3, 2], 0).is_err());
    assert!(batch.reusable_surface([3, 2], 0).is_err());
    assert!(batch.reusable_scratch_surface([3, 2]).is_err());
    assert!(batch.reusable_overwritten_surface([3, 2]).is_err());
    assert!(batch.shader_buffer(&[0]).is_err());
    assert!(batch.resources().is_empty());
    assert!(batch.commands().is_empty());
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn root_clear_color_does_not_leak_into_scratch_allocations() -> Result<()> {
    let context = gpu_support::context()?;
    let mut batch = gpu_support::batch(&context);
    let color = u32::from_le_bytes([91, 47, 13, 127]);
    let mut targets = Targets::new(&mut batch, [3, 2], color)?;
    let root = targets.main.image();
    batch.readback(root)?;
    let slot = targets.acquire(&mut batch)?;
    batch.readback(targets.get(slot)?.image())?;
    let _surface = targets.take(slot)?;
    let replacement = targets.acquire(&mut batch)?;
    batch.readback(targets.get(replacement)?.image())?;
    let output = context
        .adapter
        .submit_compute(&batch)
        .map_err(|error| format!("{error:?}"))?
        .readback()?;
    assert_eq!(
        output,
        vec![color.to_le_bytes().repeat(6), vec![0; 24], vec![0; 24]]
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_slots_reuse_only_released_allocations() -> Result<()> {
    let context = gpu_support::context()?;
    let mut batch = gpu_support::batch(&context);
    let mut targets = Targets::new(&mut batch, [3, 2], 0)?;
    assert_eq!(targets.size(), (3, 2));
    assert_eq!(targets.main.byte_len(), 24);
    let a = targets.acquire(&mut batch)?;
    let b = targets.acquire(&mut batch)?;
    let image_a = targets.get(a)?.image();
    let image_b = targets.get(b)?.image();
    assert_ne!(image_a, image_b);
    targets.release(a)?;
    assert!(targets.get(a).is_err());
    assert!(targets.release(a).is_err());
    let reused = targets.acquire(&mut batch)?;
    assert_eq!(reused, a);
    assert_eq!(targets.get(reused)?.image(), image_a);
    assert_eq!(targets.get(b)?.image(), image_b);
    assert_eq!(
        batch
            .resources()
            .iter()
            .filter(|r| matches!(r, Resource::Texture(_)))
            .count(),
        3
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn extracted_surfaces_remain_owned_and_replacement_does_not_retire_commands() -> Result<()> {
    let context = gpu_support::context()?;
    let mut batch = gpu_support::batch(&context);
    let mut targets = Targets::new(&mut batch, [2, 1], 0)?;
    let slot = targets.acquire(&mut batch)?;
    let surface = targets.take(slot)?;
    let saved = surface.image();
    assert!(targets.get(slot).is_err());
    assert!(targets.take(slot).is_err());
    assert_eq!(targets.acquire(&mut batch)?, slot);
    let replaced = targets.get(slot)?.image();
    assert_ne!(saved, replaced);
    targets.install(&mut batch, slot, surface)?;
    assert_eq!(targets.get(slot)?.image(), saved);
    assert_eq!(batch.size(replaced)?, 8);
    assert_eq!(batch.size(saved)?, 8);
    targets.release(slot)?;
    assert_eq!(
        batch
            .resources()
            .iter()
            .filter(|r| matches!(r, Resource::Texture(_)))
            .count(),
        3
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn invalid_surface_contexts_fail_without_changing_live_slots() -> Result<()> {
    let context = gpu_support::context()?;
    let mut batch = gpu_support::batch(&context);
    for size in [[0, 1], [1, 0], [u32::MAX, 1]] {
        assert!(Targets::new(&mut batch, size, 0).is_err());
    }
    assert!(batch.resources().is_empty());
    let mut targets = Targets::new(&mut batch, [2, 1], 0)?;
    let slot = targets.acquire(&mut batch)?;
    let saved = targets.get(slot)?.image();
    let other = gpu_support::context()?;
    let mut foreign = gpu_support::batch(&other);
    assert!(targets.acquire(&mut foreign).is_err());
    let wrong_owner = Surface::allocate(&mut foreign, [2, 1], 0)?;
    assert!(targets.install(&mut batch, slot, wrong_owner).is_err());
    let wrong_size = Surface::allocate(&mut batch, [1, 1], 0)?;
    assert!(targets.install(&mut batch, slot, wrong_size).is_err());
    assert_eq!(targets.get(slot)?.image(), saved);
    assert!(targets.release(RenderTargetId::Main).is_err());
    assert!(targets.take(RenderTargetId::Scratch(99)).is_err());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn capacity_target_keeps_logical_scratch_extent_and_rejects_invalid_bounds() -> Result<()> {
    let context = gpu_support::context()?;
    let mut batch = gpu_support::batch(&context);
    let texture = context.create_texture(24, 20)?;
    let image = batch.import_texture(&texture)?;
    let mut targets = Targets::from_image(&batch, image, [17, 13])?;
    assert_eq!(targets.size(), (17, 13));
    let scratch = targets.acquire(&mut batch)?;
    assert_eq!(targets.get(scratch)?.size, [17, 13]);
    for size in [[25, 13], [17, 21], [0, 13], [17, 0]] {
        assert!(
            Targets::from_image(&batch, image, size).is_err(),
            "{size:?}"
        );
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn resizing_scratch_reuses_capacity_without_changing_logical_bounds() -> Result<()> {
    assert_scratch_capacity(false)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn exact_roots_cannot_consume_scratch_capacity_during_resize() -> Result<()> {
    assert_scratch_capacity(true)
}

fn assert_scratch_capacity(owned_root: bool) -> Result<()> {
    use crate::native::runtime::compute::SurfacePool;
    use crate::{NativeBackend, NativeContext, NativeContextOptions};
    use std::{cell::RefCell, rc::Rc};

    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?
    };
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let main = context.create_texture(40, 32)?;
    let mut previous = None;
    for (index, size) in [[16, 12], [20, 14], [18, 13]].into_iter().enumerate() {
        let mut batch = ComputeBatch::with_surfaces(pool.clone());
        let image = batch.import_texture(&main)?;
        let mut targets = if owned_root {
            Targets::new(&mut batch, size, 0)?
        } else {
            Targets::from_image(&batch, image, size)?
        };
        let slot = targets.acquire(&mut batch)?;
        let scratch = targets.get(slot)?;
        assert_eq!(scratch.size, size);
        let texture = batch.persistent_texture(scratch.image())?.unwrap();
        let identity = Rc::downgrade(&texture.state);
        if index == 2 {
            assert!(
                std::rc::Weak::ptr_eq(previous.as_ref().unwrap(), &identity),
                "a smaller logical viewport must reuse the preceding scratch allocation"
            );
        }
        assert_eq!(scratch.byte_len(), batch.size(scratch.image())? as u64);
        let physical_bytes = batch.size(scratch.image())?;
        let color = if index == 1 { 0xff352f17u32 } else { 0 };
        if color != 0 {
            use crate::native::runtime::program::filter::{self, BasicFilter};
            use crate::shared::filter_config::FilterConfig;
            let extent = texture.size();
            filter::encode(
                &mut batch,
                BasicFilter::Clear,
                FilterConfig {
                    width: extent.0,
                    height: extent.1,
                    region_width: extent.0,
                    region_height: extent.1,
                    clear_color: color,
                    ..Default::default()
                },
                None,
                None,
                scratch.image(),
            )?;
        }
        batch.readback(scratch.image())?;
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        drop(targets);
        drop(batch);
        assert_eq!(
            receipt.readback()?,
            vec![color.to_le_bytes().repeat(physical_bytes / 4)]
        );
        previous = Some(identity);
    }
    context.check_validation()?;
    Ok(())
}

#[test]
fn batch_owned_pixels_cannot_become_a_retained_frame_target() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let image = batch.texture_rgba8([2, 1], vec![0; 8])?;
    assert!(Targets::from_image(&batch, image, [2, 1]).is_err());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_surfaces_import_into_later_batches_on_the_same_device() -> Result<()> {
    let context = gpu_support::context()?;
    let mut first = gpu_support::batch(&context);
    let color = 0xff231347u32;
    let surface = Surface::allocate(&mut first, [3, 2], color)?;
    let submitted = context.submit_compute(&first)?;
    drop(first);
    let mut next = gpu_support::batch(&context);
    let mut targets = Targets::new(&mut next, [3, 2], 0)?;
    let slot = targets.acquire(&mut next)?;
    targets.install(&mut next, slot, surface)?;
    next.readback(targets.get(slot)?.image())?;
    let output = context
        .adapter
        .submit_compute(&next)
        .map_err(|error| format!("{error:?}"))?
        .readback()?;
    assert_eq!(output, vec![color.to_le_bytes().repeat(6)]);
    submitted.wait()?;
    context.check_validation()?;
    Ok(())
}
