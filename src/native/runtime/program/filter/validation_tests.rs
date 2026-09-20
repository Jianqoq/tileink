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
