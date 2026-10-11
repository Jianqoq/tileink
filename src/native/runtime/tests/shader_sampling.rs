//! Sampling has a scalar bilinear oracle and retained damage checks. Separate
//! input/output textures are essential: reading a concurrently written neighbour
//! would make refraction depend on GPU scheduling.
use super::*;
use crate::native::runtime::compute::ComputeBatch;
use crate::shared::filter_config::FilterConfig;

fn sampling(offset: [f32; 4], radius: f32) -> Result<crate::ShaderPipeline> {
    Ok(ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("../../../../examples/shaders/sample.metal").into()),
            hlsl: Some(include_str!("../../../../examples/shaders/sample.hlsl").into()),
        },
        "sample",
        ShaderInput::Sampled { radius },
        4,
    )?
    .filter(offset.map(f32::to_bits))?)
}
fn sample_record(
    context: &crate::NativeContext,
    filter: &crate::ShaderPipeline,
    compact: bool,
) -> Result<Vec<u32>> {
    let mut batch = ComputeBatch::new();
    let mut pixels = vec![0xffeeddccu32; 16 * 16];
    for y in 0..5 {
        for x in 0..7 {
            pixels[y * 16 + x] = u32::from_le_bytes([x as u8 * 29, y as u8 * 41, 80, 255]);
        }
    }
    let input = batch.texture_rgba8([16, 16], bytemuck::cast_slice(&pixels).to_vec())?;
    let output = batch.texture_rgba8([16, 16], vec![0xa5; 16 * 16 * 4])?;
    crate::native::runtime::program_shader::pipeline::record_output(
        &mut batch,
        filter,
        FilterConfig {
            width: 7,
            height: 5,
            region_width: 7,
            region_height: 5,
            ..Default::default()
        },
        compact.then_some([0].as_slice()),
        &[input],
        crate::native::runtime::program_shader::Output {
            image: output,
            origin: [0, 0],
            extent: [7, 5],
        },
    )?;
    batch.readback(output)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    Ok(receipt.readback()?[0]
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_sampling_is_bilinear_bounded_transparent_and_padding_safe() -> Result<()> {
    let context = context()?;
    for offset in [[0.5, -0.25, 0.0, 0.0], [-99.0, 99.0, 0.0, 0.0], [0.0; 4]] {
        let filter = sampling(offset, 1.0)?;
        for compact in [false, true] {
            let actual = sample_record(&context, &filter, compact)?;
            for y in 0..16 {
                for x in 0..16 {
                    if x >= 7 || y >= 5 {
                        assert_eq!(actual[y * 16 + x], 0xa5a5a5a5);
                        continue;
                    }
                    let sx = x as f64 + f64::from(offset[0].clamp(-1.0, 1.0));
                    let sy = y as f64 + f64::from(offset[1].clamp(-1.0, 1.0));
                    let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
                    let (tx, ty) = (sx - sx.floor(), sy - sy.floor());
                    let mut expected = [0.0; 4];
                    for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
                        for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
                            let (px, py) = (ix + dx, iy + dy);
                            if (0..7).contains(&px) && (0..5).contains(&py) {
                                for (v, s) in expected.iter_mut().zip([
                                    px as f64 * 29.0,
                                    py as f64 * 41.0,
                                    80.0,
                                    255.0,
                                ]) {
                                    *v += s / 255.0 * wx * wy;
                                }
                            }
                        }
                    }
                    close(actual[y * 16 + x], pack(expected));
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[path = "shader_sampling_pixels.rs"]
mod pixels;
