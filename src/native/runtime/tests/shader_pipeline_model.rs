use super::*;
pub(super) fn pipeline(format: ShaderTextureFormat) -> Result<ShaderPipeline> {
    use ShaderBindingKind as K;
    let sources = ShaderSources {
        metal: Some(include_str!("shaders/pipeline.metal").into()),
        hlsl: Some(include_str!("shaders/pipeline.hlsl").replace(
            "rgba16f",
            if format == ShaderTextureFormat::Rgba16Float {
                "rgba16f"
            } else {
                "rgba32f"
            },
        )),
    };
    let kernel = |entry, group, bindings: &[(u32, &str, K)]| {
        ShaderKernel::new(
            sources.clone(),
            entry,
            group,
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
    let fill = kernel(
        "fill_buffer",
        [2, 2, 2],
        &[(4, "storage", K::ReadWriteBuffer)],
    )?;
    let down = kernel(
        "down_hdr",
        [8, 8, 1],
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "intermediate", K::WriteTexture(format)),
        ],
    )?;
    let copy = kernel(
        "copy_hdr",
        [8, 8, 1],
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "intermediate", K::WriteTexture(format)),
        ],
    )?;
    let inplace = kernel(
        "inplace_hdr",
        [8, 8, 1],
        &[
            (0, "config", K::Config),
            (2, "intermediate", K::ReadWriteTexture(format)),
        ],
    )?;
    let finish = kernel(
        "finish_hdr",
        [8, 8, 1],
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (
                3,
                "output_image",
                K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            ),
            (4, "storage_read", K::ReadBuffer),
            (5, "tint", K::Texture),
            (6, "linear_sampler", K::Sampler),
        ],
    )?;
    let half = ShaderExtent::downsample(2);
    Ok(ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Output,
            ShaderResource::config(half),
            ShaderResource::config(ShaderExtent::FULL),
            ShaderResource::Texture {
                extent: half,
                format,
            },
            ShaderResource::Texture {
                extent: half,
                format,
            },
            ShaderResource::Texture {
                extent: half,
                format,
            },
            ShaderResource::Buffer(vec![0; 64].into()),
            ShaderResource::Image {
                size: [1, 1],
                format: ShaderTextureFormat::Rgba8Unorm,
                bytes: Rc::from([128, 255, 0, 255]),
            },
            ShaderResource::Sampler(ShaderSampler::Linear),
        ],
        vec![
            ShaderPass {
                kernel: fill,
                bindings: vec![(4, 7)],
                dispatch: ShaderDispatch::Groups([2, 2, 2]),
            },
            ShaderPass {
                kernel: down,
                bindings: vec![(0, 2), (1, 0), (2, 4)],
                dispatch: ShaderDispatch::Image(half),
            },
            ShaderPass {
                kernel: copy.clone(),
                bindings: vec![(0, 2), (1, 4), (2, 5)],
                dispatch: ShaderDispatch::Image(half),
            },
            ShaderPass {
                kernel: copy,
                bindings: vec![(0, 2), (1, 5), (2, 6)],
                dispatch: ShaderDispatch::Image(half),
            },
            ShaderPass {
                kernel: inplace,
                bindings: vec![(0, 2), (2, 6)],
                dispatch: ShaderDispatch::Image(half),
            },
            ShaderPass {
                kernel: finish,
                bindings: vec![(0, 3), (1, 6), (3, 1), (4, 7), (5, 8), (6, 9)],
                dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
            },
        ],
    )?)
}
pub(super) fn source_pixels(size: [u32; 2]) -> Vec<u32> {
    (0..size[0] * size[1])
        .map(|i| {
            u32::from_le_bytes([
                (i % size[0] * 11 % 256) as u8,
                (i / size[0] * 23 % 256) as u8,
                31,
                255,
            ])
        })
        .collect()
}
pub(super) fn check(actual: &[u32], source: &[u32], size: [u32; 2], tint: u8) {
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut expected = source[((y / 2 * 2) * size[0] + x / 2 * 2) as usize].to_le_bytes();
            let factor = (((x + y * size[0]) % 64 + 1) % 4 + 1) as f32 * 0.25;
            expected[0] = (f32::from(expected[0]) * factor * f32::from(tint) / 255.0).round() as u8;
            let pixel = actual[(y * size[0] + x) as usize].to_le_bytes();
            assert!(
                pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "{x},{y}: {pixel:?} != {expected:?}"
            );
        }
    }
}
