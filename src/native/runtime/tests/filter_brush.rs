use super::*;
use crate::{native::runtime::renderer::Images, shared::layer::filter::Filter};
#[test]
fn brush_dispatch_rejects_nonrecord_offsets_aliases_and_foreign_batches() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let empty = crate::shared::image_resource::ImageResourceStore::default();
    let upload = empty.upload_merged(&empty, 4096, 4, 16, None);
    let images = Images::record(&mut batch, &upload)?;
    let filter = Filter::Flood {
        brush: peniko::Color::from_rgb8(31, 57, 111).into(),
    };
    let prepared = crate::render::filter_resources::upload::FilterResourceUpload::new(
        &[],
        Some(&filter),
        Some(&upload),
    );
    let brushes = Brushes::record(&mut batch, prepared.brushes, upload.textures().len())?.unwrap();
    let target = batch.texture_rgba8([2, 2], vec![0; 16])?;
    let c = FilterConfig {
        width: 2,
        height: 2,
        region_width: 2,
        region_height: 2,
        ..Default::default()
    };
    // SAFETY: every call uses the exact immutable image upload used by record.
    unsafe {
        assert!(
            brushes
                .encode(
                    &mut batch,
                    FilterConfig {
                        brush_offset: 1,
                        ..c
                    },
                    None,
                    images.textures(),
                    None,
                    target
                )
                .is_err()
        );
        assert!(batch.passes().is_empty());
        assert!(
            brushes
                .encode(&mut batch, c, None, images.textures(), Some(target), target)
                .is_err()
        );
        assert!(batch.passes().is_empty());
        brushes.encode(&mut batch, c, None, images.textures(), None, target)?;
        assert_eq!(batch.passes().len(), 1);
        let mut foreign = ComputeBatch::new();
        let output = foreign.texture_rgba8([2, 2], vec![0; 16])?;
        assert!(
            brushes
                .encode(&mut foreign, c, None, images.textures(), None, output)
                .is_err()
        );
        assert!(foreign.passes().is_empty());
    }
    assert!(
        Brushes::record(
            &mut batch,
            GpuBrushUpload::default(),
            upload.textures().len()
        )?
        .is_none()
    );
    Ok(())
}
