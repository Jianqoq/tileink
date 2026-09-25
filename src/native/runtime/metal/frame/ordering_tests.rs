//! Separate compute and blit encoders preserve read/write ordering and pixel data.
use super::*;
use crate::native::runtime::{
    compute::TextureCopy,
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn compute_runs_preserve_texture_dependencies() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for (width, height) in [(1, 1), (17, 15), (257, 3)] {
        for count in [1, 2, 17] {
            for blit in [false, true] {
                let mut batch = ComputeBatch::new();
                let bytes = (width * height * 4) as usize;
                let a = batch.texture_rgba8([width, height], vec![0xa5; bytes])?;
                let b = batch.texture_rgba8([width, height], vec![0x5a; bytes])?;
                let config = FilterConfig {
                    width,
                    height,
                    region_width: width,
                    region_height: height,
                    ..Default::default()
                };
                for step in 0..count {
                    filter::encode(
                        &mut batch,
                        BasicFilter::Clear,
                        FilterConfig {
                            clear_color: u32::from_le_bytes([3, 7, 11, 128 + step]),
                            ..config
                        },
                        None,
                        None,
                        a,
                    )?;
                    if blit {
                        batch.copy_texture(TextureCopy {
                            source: a,
                            destination: b,
                            source_origin: [0; 3],
                            destination_origin: [0; 3],
                            extent: [width, height, 1],
                        })?;
                    } else {
                        filter::encode(&mut batch, BasicFilter::Copy, config, None, Some(a), b)?;
                    }
                    filter::encode(
                        &mut batch,
                        BasicFilter::SourceAlpha,
                        config,
                        None,
                        Some(b),
                        a,
                    )?;
                    // Overwrite a previously read texture, rebinding a different
                    // uniform. The alpha copied into A must remain unchanged.
                    filter::encode(
                        &mut batch,
                        BasicFilter::Clear,
                        FilterConfig {
                            clear_color: u32::from_le_bytes([1, 2, 3, 255]),
                            ..config
                        },
                        None,
                        None,
                        b,
                    )?;
                }
                batch.readback(a)?;
                batch.readback(b)?;
                for pass in batch.passes() {
                    pipeline::ensure(
                        &metal.device,
                        &mut metal.libraries,
                        &mut metal.pipelines,
                        pass.shader,
                    )?;
                }
                let frame = Frame::record(&metal, &batch)?;
                frame.command.commit();
                frame.wait()?;
                assert_eq!(
                    frame.readback()?,
                    vec![
                        [0, 0, 0, 127 + count].repeat(bytes / 4),
                        [1, 2, 3, 255].repeat(bytes / 4),
                    ]
                );
            }
        }
    }
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn independent_compute_writes_preserve_both_outputs() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    let mut batch = ComputeBatch::new();
    let a = batch.texture_rgba8([17, 15], vec![0; 17 * 15 * 4])?;
    let b = batch.texture_rgba8([17, 15], vec![0; 17 * 15 * 4])?;
    let config = FilterConfig {
        width: 17,
        height: 15,
        region_width: 17,
        region_height: 15,
        ..Default::default()
    };
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        FilterConfig {
            clear_color: u32::from_le_bytes([3, 7, 11, 255]),
            ..config
        },
        None,
        None,
        a,
    )?;
    filter::encode(
        &mut batch,
        BasicFilter::Clear,
        FilterConfig {
            clear_color: u32::from_le_bytes([13, 17, 19, 255]),
            ..config
        },
        None,
        None,
        b,
    )?;
    batch.readback(a)?;
    batch.readback(b)?;
    for pass in batch.passes() {
        pipeline::ensure(
            &metal.device,
            &mut metal.libraries,
            &mut metal.pipelines,
            pass.shader,
        )?;
    }
    let frame = Frame::record(&metal, &batch)?;
    frame.command.commit();
    frame.wait()?;
    assert_eq!(
        frame.readback()?,
        vec![
            [3, 7, 11, 255].repeat(17 * 15),
            [13, 17, 19, 255].repeat(17 * 15),
        ]
    );
    metal.assert_valid()
}
