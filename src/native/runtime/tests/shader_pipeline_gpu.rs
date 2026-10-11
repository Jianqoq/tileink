//! Production-native pipeline tests: HDR, extra textures/samplers, read/write
//! buffers, barriers, 3D groups, odd extents, reuse, backdrop and retained updates.
use super::compute::{ComputeBatch, SurfacePool};
use super::{Result, brush_support::context};
use crate::*;
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::{cell::RefCell, rc::Rc};

#[path = "shader_pipeline_model.rs"]
mod model;
use model::{check, pipeline, source_pixels};
#[path = "shader_pipeline_graph.rs"]
mod graph;
#[path = "shader_pipeline_retained.rs"]
mod retained;
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_hdr_multiresources_and_barriers_reuse_correctly() -> Result<()> {
    let context = context()?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    for format in [
        ShaderTextureFormat::Rgba16Float,
        ShaderTextureFormat::Rgba32Float,
    ] {
        let pipeline = pipeline(format)?;
        for size in [[17, 9], [31, 17], [17, 9], [1, 1]] {
            for phase in 0..3 {
                let tint = if phase == 1 { 64 } else { 128 };
                let (image_format, bytes): (_, Rc<[u8]>) = match phase {
                    1 => (
                        ShaderTextureFormat::Rgba16Float,
                        [0x3400u16, 0x3c00, 0, 0x3c00]
                            .into_iter()
                            .flat_map(u16::to_le_bytes)
                            .collect(),
                    ),
                    2 => (
                        ShaderTextureFormat::Rgba32Float,
                        [f32::from(tint) / 255.0, 1.0, 0.0, 1.0]
                            .into_iter()
                            .flat_map(f32::to_le_bytes)
                            .collect(),
                    ),
                    _ => (
                        ShaderTextureFormat::Rgba8Unorm,
                        Rc::from([tint, 255, 0, 255]),
                    ),
                };
                let pipeline = pipeline.with_resource(
                    8,
                    ShaderResource::Image {
                        size: [1, 1],
                        format: image_format,
                        bytes,
                    },
                )?;
                let mut batch = ComputeBatch::with_surfaces(pool.clone());
                let pixels = source_pixels(size);
                let input = batch.texture_rgba8(size, bytemuck::cast_slice(&pixels).to_vec())?;
                let output = batch.texture_rgba8(size, vec![0; pixels.len() * 4])?;
                super::program_shader::record_pipeline(
                    &mut batch,
                    &pipeline,
                    crate::shared::filter_config::FilterConfig {
                        width: size[0],
                        height: size[1],
                        region_width: size[0],
                        region_height: size[1],
                        ..Default::default()
                    },
                    None,
                    &[input],
                    output,
                )?;
                // A and C have disjoint lifetimes and share one lease; B stays separate.
                let hdr:std::collections::BTreeSet<_>=batch.passes().iter().flat_map(|p|p.bindings.iter()).filter_map(|(binding,id)| (binding.kind.writable() && matches!(&batch.resources()[id.index()],super::compute::Resource::Texture(t) if t.format==format)).then_some(id.index())).collect();
                assert_eq!(hdr.len(), 2);
                batch.readback(output)?;
                let receipt = context
                    .adapter
                    .submit_compute(&batch)
                    .map_err(|e| format!("{e:?}"))?;
                let bytes = receipt.readback()?;
                let actual: Vec<_> = bytes[0]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .copied()
                    .map(u32::from_le_bytes)
                    .collect();
                check(&actual, &pixels, size, tint);
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_region_dispatch_preserves_phase_and_outside_pixels() -> Result<()> {
    let context = context()?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let original = pipeline(ShaderTextureFormat::Rgba16Float)?;
    let mut passes = original.passes.to_vec();
    for p in &mut passes {
        if let ShaderDispatch::Image(extent) = p.dispatch {
            p.dispatch = ShaderDispatch::Region {
                extent,
                outset: if p.kernel.entry.as_ref() == "finish_hdr" {
                    0
                } else {
                    4
                },
            };
        }
    }
    let resources = original.resources.iter().cloned().collect();
    let pipeline = ShaderPipeline::new(resources, passes)?;
    let size = [17, 9];
    let pixels = source_pixels(size);
    let mut batch = ComputeBatch::with_surfaces(pool);
    let input = batch.texture_rgba8(size, bytemuck::cast_slice(&pixels).to_vec())?;
    let output = batch.texture_rgba8(size, vec![0xa5; pixels.len() * 4])?;
    super::program_shader::record_pipeline(
        &mut batch,
        &pipeline,
        crate::shared::filter_config::FilterConfig {
            width: 17,
            height: 9,
            region_x0: 4,
            region_y0: 2,
            region_width: 11,
            region_height: 6,
            ..Default::default()
        },
        None,
        &[input],
        output,
    )?;
    batch.readback(output)?;
    let bytes = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?
        .readback()?;
    let actual: Vec<_> = bytes[0]
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect();
    for y in 0..9 {
        for x in 0..17 {
            if !(4..15).contains(&x) || !(2..8).contains(&y) {
                assert_eq!(actual[y * 17 + x], 0xa5a5a5a5);
            }
        }
    }
    // Check the same scalar reference only over output; outside bytes are sentinels.
    let mut reference = pixels.clone();
    for y in 2..8 {
        for x in 4..15 {
            let mut p = pixels[(y / 2 * 2) * 17 + x / 2 * 2].to_le_bytes();
            let factor = (((x + y * 17) % 64 + 1) % 4 + 1) as f32 * 0.25;
            p[0] = (f32::from(p[0]) * factor * 128.0 / 255.0).round() as u8;
            reference[y * 17 + x] = u32::from_le_bytes(p);
            assert!(
                actual[y * 17 + x]
                    .to_le_bytes()
                    .iter()
                    .zip(reference[y * 17 + x].to_le_bytes())
                    .all(|(a, b)| a.abs_diff(b) <= 1)
            );
        }
    }
    context.check_validation()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_multiple_writable_images_preserve_distinct_results() -> Result<()> {
    use ShaderBindingKind as K;
    let context = context()?;
    let sources = ShaderSources {
        metal: Some(include_str!("shaders/pipeline.metal").into()),
        hlsl: Some(include_str!("shaders/pipeline.hlsl").into()),
    };
    let f = ShaderTextureFormat::Rgba16Float;
    let kernel = |entry, bindings: &[(u32, &str, K)]| {
        ShaderKernel::new(
            sources.clone(),
            entry,
            [8, 8, 1],
            bindings
                .iter()
                .map(|&(slot, name, kind)| ShaderBinding {
                    slot,
                    name: name.into(),
                    kind,
                })
                .collect(),
        )
    };
    let split = kernel(
        "split_hdr",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "intermediate", K::WriteTexture(f)),
            (7, "alternate", K::WriteTexture(f)),
        ],
    )?;
    let join = kernel(
        "join_hdr",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (7, "alternate_read", K::Texture),
            (
                3,
                "output_image",
                K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            ),
        ],
    )?;
    let pipeline = ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
            ShaderResource::Texture {
                extent: ShaderExtent::FULL,
                format: f,
            },
            ShaderResource::Texture {
                extent: ShaderExtent::FULL,
                format: f,
            },
        ],
        vec![
            ShaderPass {
                kernel: split,
                bindings: vec![(0, 2), (1, 0), (2, 3), (7, 4)],
                dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
            },
            ShaderPass {
                kernel: join,
                bindings: vec![(0, 2), (1, 3), (7, 4), (3, 1)],
                dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
            },
        ],
    )?;
    let mut canvas = Canvas::new(17, 9, 1.0);
    canvas.push_filter_layer(
        pipeline.into(),
        Region::rect(Rect::new(0.0, 0.0, 17.0, 9.0), Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 17.0, 9.0),
        Radius::ZERO,
        Color::from_rgb8(180, 80, 31),
    );
    canvas.pop_layer();
    let mut renderer = NativeRenderer::with_context(&context, 17, 9)?;
    let image = renderer.render_to_image(&canvas)?.readback()?;
    for p in image.pixels {
        assert!(
            p.to_le_bytes()
                .iter()
                .zip([180, 80, 31, 255])
                .all(|(a, b)| a.abs_diff(b) <= 1)
        );
    }
    context.check_validation()?;
    Ok(())
}
