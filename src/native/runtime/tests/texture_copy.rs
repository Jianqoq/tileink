use super::*;

#[test]
fn texture_copy_checks_resource_ownership_kind_and_every_axis() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_array_rgba8([4, 3, 2], vec![0; 4 * 3 * 2 * 4])?;
    let destination = batch.texture_array_rgba8([5, 4, 3], vec![0; 5 * 4 * 3 * 4])?;
    let valid = TextureCopy {
        source,
        destination,
        source_origin: [1, 1, 0],
        destination_origin: [2, 2, 1],
        extent: [3, 2, 2],
    };
    batch.copy_texture(valid)?;
    assert_eq!(batch.commands().len(), 1);
    for axis in 0..3 {
        let mut invalid = valid;
        invalid.source_origin[axis] = u32::MAX;
        assert!(batch.copy_texture(invalid).is_err());
        invalid = valid;
        invalid.destination_origin[axis] += 1;
        assert!(batch.copy_texture(invalid).is_err());
    }
    let buffer = batch.buffer(vec![0; 4])?;
    assert!(
        batch
            .copy_texture(TextureCopy {
                source: buffer,
                ..valid
            })
            .is_err()
    );
    assert!(
        batch
            .copy_texture(TextureCopy {
                destination: source,
                ..valid
            })
            .is_err()
    );
    let foreign = ComputeBatch::new().texture_rgba8([4, 3], vec![0; 4 * 3 * 4])?;
    assert!(
        batch
            .copy_texture(TextureCopy {
                source: foreign,
                ..valid
            })
            .is_err()
    );
    assert_eq!(batch.commands().len(), 1);
    batch.copy_texture(TextureCopy {
        extent: [0, 2, 2],
        ..valid
    })?;
    assert_eq!(batch.commands().len(), 1);
    Ok(())
}

#[test]
fn copies_and_dispatches_retain_recording_order() -> Result<()> {
    use crate::native::runtime::program::filter::{self, BasicFilter};
    use crate::shared::filter_config::FilterConfig;
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let destination = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let copy = TextureCopy {
        source,
        destination,
        source_origin: [0; 3],
        destination_origin: [0; 3],
        extent: [1; 3],
    };
    batch.copy_texture(copy)?;
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        FilterConfig {
            width: 1,
            height: 1,
            region_width: 1,
            region_height: 1,
            ..Default::default()
        },
        None,
        None,
        source,
    )?;
    batch.copy_texture(copy)?;
    assert!(matches!(
        batch.commands(),
        [
            Command::CopyTexture(_),
            Command::Dispatch(0),
            Command::CopyTexture(_)
        ]
    ));
    assert_eq!(batch.passes().len(), 1);
    assert!(batch.outputs().is_empty());
    Ok(())
}
