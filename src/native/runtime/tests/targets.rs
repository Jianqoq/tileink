use super::*;

#[test]
fn root_clear_color_does_not_leak_into_scratch_allocations() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let color = u32::from_le_bytes([91, 47, 13, 127]);
    let mut targets = Targets::new(&mut batch, [3, 2], color)?;
    let root = targets.main.image().index();
    assert_eq!(batch.resources()[root].bytes(), [91, 47, 13, 127].repeat(6));
    let slot = targets.acquire(&mut batch)?;
    let scratch = targets.get(slot)?.image().index();
    assert_eq!(batch.resources()[scratch].bytes(), [0; 24]);
    let _surface = targets.take(slot)?;
    let replacement = targets.acquire(&mut batch)?;
    assert_eq!(
        batch.resources()[targets.get(replacement)?.image().index()].bytes(),
        [0; 24]
    );
    assert_eq!(batch.resources()[root].bytes(), [91, 47, 13, 127].repeat(6));
    Ok(())
}

#[test]
fn nested_slots_reuse_only_released_allocations() -> Result<()> {
    let mut batch = ComputeBatch::new();
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
    assert_eq!(batch.resources().len(), 3);
    Ok(())
}

#[test]
fn extracted_surfaces_remain_owned_and_replacement_does_not_retire_commands() -> Result<()> {
    let mut batch = ComputeBatch::new();
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
    assert_eq!(batch.resources().len(), 3);
    Ok(())
}

#[test]
fn invalid_surface_contexts_fail_without_changing_live_slots() -> Result<()> {
    let mut batch = ComputeBatch::new();
    for size in [[0, 1], [1, 0], [u32::MAX, 1]] {
        assert!(Targets::new(&mut batch, size, 0).is_err());
    }
    assert!(batch.resources().is_empty());
    let mut targets = Targets::new(&mut batch, [2, 1], 0)?;
    let slot = targets.acquire(&mut batch)?;
    let saved = targets.get(slot)?.image();
    let mut foreign = ComputeBatch::new();
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
fn capacity_target_keeps_logical_scratch_extent_and_rejects_invalid_bounds() {
    let mut batch = ComputeBatch::new();
    let image = batch.texture_rgba8([24, 20], vec![0; 24 * 20 * 4]).unwrap();
    let mut targets = Targets::from_image(&batch, image, [17, 13]).unwrap();
    assert_eq!(targets.size(), (17, 13));
    let scratch = targets.acquire(&mut batch).unwrap();
    assert_eq!(targets.get(scratch).unwrap().size, [17, 13]);
    for size in [[25, 13], [17, 21], [0, 13], [17, 0]] {
        assert!(
            Targets::from_image(&batch, image, size).is_err(),
            "{size:?}"
        );
    }
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
