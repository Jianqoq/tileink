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
