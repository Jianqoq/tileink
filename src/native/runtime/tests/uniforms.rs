use super::*;
use crate::native::runtime::program::filter::{self, BasicFilter};
use crate::shared::filter_config::FilterConfig;

fn batch() -> Result<ComputeBatch> {
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([2, 2], vec![0; 16])?;
    for width in [1, 2] {
        filter::encode(
            &mut batch,
            BasicFilter::Clear,
            FilterConfig {
                width: 2,
                height: 2,
                region_width: width,
                region_height: 2,
                ..Default::default()
            },
            None,
            None,
            target,
        )?;
    }
    Ok(batch)
}

#[test]
fn immutable_uniform_slots_are_aligned_distinct_and_zero_padded() -> Result<()> {
    let batch = batch()?;
    for alignment in [4, 256, 1024] {
        let packed = Uniforms::new(&batch, alignment)?;
        let mut previous_end = 0;
        let mut count = 0;
        for (index, offset) in packed.offsets.iter().enumerate() {
            let Some(offset) = offset else { continue };
            let offset = *offset as usize;
            let data = batch.resources()[index].bytes();
            assert_eq!(offset % alignment, 0);
            assert_eq!(offset, previous_end);
            assert_eq!(&packed.bytes[offset..offset + data.len()], data);
            previous_end = offset + data.len().div_ceil(alignment) * alignment;
            assert!(
                packed.bytes[offset + data.len()..previous_end]
                    .iter()
                    .all(|&byte| byte == 0)
            );
            count += 1;
        }
        assert_eq!(count, 2);
        assert_eq!(previous_end, packed.bytes.len());
    }
    Ok(())
}

#[test]
fn readback_and_storage_aliases_are_never_packed_as_immutable_constants() -> Result<()> {
    let mut batch = batch()?;
    let (binding, first) = *batch.passes()[0]
        .bindings
        .iter()
        .find(|(binding, _)| binding.kind == BindingKind::Uniform)
        .unwrap();
    let second = batch.passes()[1]
        .bindings
        .iter()
        .find(|(binding, _)| binding.kind == BindingKind::Uniform)
        .unwrap()
        .1;
    batch.readback(first)?;
    // Model use by another storage-reading/writing kernel. Classification must
    // inspect every use, not just the first constant binding in the frame.
    for kind in [BindingKind::Read, BindingKind::Write] {
        batch.passes[1]
            .bindings
            .push((crate::native::shaders::Binding { kind, ..binding }, second));
        let packed = Uniforms::new(&batch, 256)?;
        assert!(packed.bytes.is_empty());
        assert!(packed.offsets.iter().all(Option::is_none));
        batch.passes[1].bindings.pop();
    }
    Ok(())
}

#[test]
fn empty_unused_and_invalid_uniform_layouts_are_handled() -> Result<()> {
    let mut batch = ComputeBatch::new();
    batch.buffer(vec![7; 16])?;
    assert!(Uniforms::new(&batch, 256)?.bytes.is_empty());
    assert!(Uniforms::new(&batch, 0).is_err());
    assert!(Uniforms::new(&batch, 3).is_err());
    Ok(())
}
