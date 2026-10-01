use super::{layer, rectangle, stack, surface};
use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::{filter_config::FilterConfig, gpu_coarse::LayerStackRecord};

fn pixel_config() -> FilterConfig {
    FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        kernel_columns: 1,
        kernel_rows: 1,
        ..Default::default()
    }
}

#[test]
fn filter_rounding_operand_requires_positive_zero_before_recording() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let resources = batch.resources().len();
    for rounding_zero in [-0.0, 1.0, -1.0, f32::NAN, f32::INFINITY] {
        let config = FilterConfig {
            rounding_zero,
            ..pixel_config()
        };
        assert!(
            super::encode(
                &mut batch,
                super::BasicFilter::Clear,
                config,
                None,
                None,
                target
            )
            .is_err()
        );
        assert!(batch.passes().is_empty());
        assert_eq!(batch.resources().len(), resources);
    }
    super::encode(
        &mut batch,
        super::BasicFilter::Clear,
        pixel_config(),
        None,
        None,
        target,
    )?;
    assert_eq!(batch.passes().len(), 1);
    Ok(())
}

#[test]
fn direct_composite_ignores_sdf_geometry_but_surface_bounds_are_checked() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let config = FilterConfig {
        rect_x0: f32::NAN,
        ..pixel_config()
    };
    rectangle::encode(
        &mut batch,
        rectangle::RectanglePass::Direct { source, mask: None },
        config,
        None,
        target,
    )?;
    assert_eq!(batch.passes().len(), 1);
    assert!(
        rectangle::encode(
            &mut batch,
            rectangle::RectanglePass::Mask,
            config,
            None,
            target
        )
        .is_err()
    );
    let invalid = FilterConfig {
        offset_x: i32::MIN,
        ..pixel_config()
    };
    assert!(surface::encode(&mut batch, invalid, None, source, target).is_err());
    assert_eq!(
        batch.passes().len(),
        1,
        "invalid inputs must not record work"
    );
    surface::encode(&mut batch, pixel_config(), None, source, target)?;
    assert_eq!(batch.passes().len(), 2);
    Ok(())
}

#[test]
fn layer_upload_rejects_invalid_logical_ranges_and_opacity_before_allocating() -> Result<()> {
    let mut batch = ComputeBatch::new();
    assert!(
        layer::Geometry::upload(
            &mut batch,
            layer::Scene {
                backdrops: &[0],
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(batch.resources().is_empty());
    let geometry = layer::Geometry::upload(&mut batch, layer::Scene::default())?;
    let count = batch.resources().len();
    let invalid = LayerStackRecord {
        tag: crate::shared::gpu_types::GPU_LAYER_OPACITY,
        draw: 0,
        payload: 256,
    };
    assert!(stack::Stack::upload(&mut batch, geometry, &[invalid]).is_err());
    assert_eq!(batch.resources().len(), count);
    let valid = LayerStackRecord {
        payload: 255,
        ..invalid
    };
    let _ = stack::Stack::upload(&mut batch, geometry, &[valid])?;
    assert_eq!(batch.resources().len(), count + 1);
    Ok(())
}

fn active_buffer(batch: &ComputeBatch, pass: usize) -> super::ResourceId {
    batch.passes()[pass]
        .bindings
        .iter()
        .find(|(binding, _)| binding.slot == 8)
        .unwrap()
        .1
}

fn tiled_config(size: [u32; 2]) -> FilterConfig {
    FilterConfig {
        width: size[0],
        height: size[1],
        region_width: size[0],
        region_height: size[1],
        ..Default::default()
    }
}

#[test]
fn filter_passes_share_validated_tile_buffers_by_contents_and_preserve_order() -> Result<()> {
    use crate::native::runtime::compute::Resource;
    let mut batch = ComputeBatch::new();
    let first = batch.texture_rgba8([64, 64], vec![0; 64 * 64 * 4])?;
    let second = batch.texture_rgba8([64, 64], vec![0; 64 * 64 * 4])?;
    let tiles = [15u32, 0, 5, 14];
    super::encode(
        &mut batch,
        super::BasicFilter::Clear,
        tiled_config([64, 64]),
        Some(&tiles),
        None,
        first,
    )?;
    let active = active_buffer(&batch, 0);
    let count = batch.resources().len();
    // Independently owned but equal lists must reuse validation and GPU storage.
    let equal = tiles.to_vec();
    super::encode(
        &mut batch,
        super::BasicFilter::Copy,
        tiled_config([64, 64]),
        Some(&equal),
        Some(first),
        second,
    )?;
    assert_eq!(active_buffer(&batch, 1), active);
    assert_eq!(
        batch.resources().len(),
        count + 1,
        "only the pass uniform is new"
    );
    let reordered = [0u32, 5, 14, 15];
    super::encode(
        &mut batch,
        super::BasicFilter::Clear,
        tiled_config([64, 64]),
        Some(&reordered),
        None,
        first,
    )?;
    assert_ne!(active_buffer(&batch, 2), active);
    let Resource::Buffer(bytes) = &batch.resources()[active.index()] else {
        panic!("tile list must be owned read-only bytes")
    };
    let expected: Vec<_> = tiles.into_iter().flat_map(u32::to_le_bytes).collect();
    assert_eq!(bytes, &expected);
    Ok(())
}

#[test]
fn cached_tile_validation_rechecks_content_bounds_and_preserves_failure_atomicity() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([64, 64], vec![0; 64 * 64 * 4])?;
    let mut tiles = [15u32, 0, 5, 14];
    super::encode(
        &mut batch,
        super::BasicFilter::Clear,
        tiled_config([64, 64]),
        Some(&tiles),
        None,
        target,
    )?;
    let resources = batch.resources().len();
    for (size, invalid) in [
        ([32, 32], tiles),
        ([64, 64], [15, 0, 5, 0]),
        ([64, 64], [16, 0, 5, 14]),
    ] {
        assert!(
            super::encode(
                &mut batch,
                super::BasicFilter::Clear,
                tiled_config(size),
                Some(&invalid),
                None,
                target
            )
            .is_err()
        );
        assert_eq!(batch.resources().len(), resources);
        assert_eq!(batch.passes().len(), 1);
    }
    tiles[3] = 0;
    assert!(
        super::encode(
            &mut batch,
            super::BasicFilter::Clear,
            tiled_config([64, 64]),
            Some(&tiles),
            None,
            target
        )
        .is_err(),
        "same address cannot bypass duplicate detection"
    );
    let malformed = FilterConfig {
        dispatch_width: 65536,
        ..tiled_config([64, 64])
    };
    assert!(
        super::encode(
            &mut batch,
            super::BasicFilter::Clear,
            malformed,
            Some(&[1, 2, 3]),
            None,
            target
        )
        .is_err()
    );
    assert_eq!(batch.resources().len(), resources);
    assert_eq!(batch.passes().len(), 1);
    let empty = [0u32; 0];
    super::encode(
        &mut batch,
        super::BasicFilter::Clear,
        tiled_config([64, 64]),
        Some(&empty),
        None,
        target,
    )?;
    let zero_region = FilterConfig {
        region_width: 0,
        ..tiled_config([64, 64])
    };
    assert!(
        super::encode(
            &mut batch,
            super::BasicFilter::Clear,
            zero_region,
            Some(&[0, 0]),
            None,
            target
        )
        .is_err()
    );
    assert_eq!(
        batch.resources().len(),
        resources,
        "empty selections and invalid regions allocate no GPU resources"
    );
    assert_eq!(batch.passes().len(), 1);
    Ok(())
}
