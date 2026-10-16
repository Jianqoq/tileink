//! A separable Gaussian with reduced-resolution RGBA16F intermediates and a
//! second application image. All effect math is application-owned native source.
use std::rc::Rc;
use tileink::{
    ShaderBinding, ShaderBindingKind as K, ShaderDispatch, ShaderError, ShaderExtent, ShaderKernel,
    ShaderPass, ShaderPipeline, ShaderResource as R, ShaderSampler, ShaderSources,
    ShaderTextureFormat as F,
};
pub fn pipeline(factor: u32, sigma: f32, tint: [u8; 4]) -> Result<ShaderPipeline, ShaderError> {
    let sources = ShaderSources {
        metal: Some(include_str!("../shaders/blur_pipeline.metal").into()),
        hlsl: Some(include_str!("../shaders/blur_pipeline.hlsl").into()),
    };
    let kernel = |entry, bindings: &[(u32, &str, K)]| {
        ShaderKernel::new(
            sources.clone(),
            entry,
            [16, 16, 1],
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
    let reduce = kernel(
        "reduce",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "low_image", K::WriteTexture(F::Rgba16Float)),
            (4, "parameters", K::ReadBuffer),
        ],
    )?;
    let horizontal = kernel(
        "horizontal",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "low_image", K::WriteTexture(F::Rgba16Float)),
            (4, "weights", K::ReadBuffer),
        ],
    )?;
    let vertical = kernel(
        "vertical",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (2, "low_image", K::WriteTexture(F::Rgba16Float)),
            (4, "weights", K::ReadBuffer),
        ],
    )?;
    let expand = kernel(
        "expand",
        &[
            (0, "config", K::Config),
            (1, "input_image", K::Texture),
            (3, "output_image", K::WriteTexture(F::Rgba8Unorm)),
            (4, "parameters", K::ReadBuffer),
            (5, "tint", K::Texture),
            (6, "linear_sampler", K::Sampler),
        ],
    )?;
    let sigma = (sigma / factor.max(1) as f32).max(0.01);
    let radius = (sigma * 3.0).ceil() as i32;
    let coefficients: Vec<_> = (-radius..=radius)
        .map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp())
        .collect();
    let sum: f32 = coefficients.iter().sum();
    let weights: Rc<[u32]> = std::iter::once(radius as u32)
        .chain(coefficients.into_iter().map(|w| (w / sum).to_bits()))
        .collect();
    let low = ShaderExtent::downsample(factor);
    let reach = radius as u32 * factor;
    ShaderPipeline::new(
        vec![
            R::Input(0),
            R::Output,
            R::Texture {
                extent: low,
                format: F::Rgba16Float,
            },
            R::Texture {
                extent: low,
                format: F::Rgba16Float,
            },
            R::config(low),
            R::config(ShaderExtent::FULL),
            R::Buffer(Rc::from([factor])),
            R::Buffer(weights),
            R::Image {
                size: [1, 1],
                format: F::Rgba8Unorm,
                bytes: Rc::from(tint),
            },
            R::Sampler(ShaderSampler::Linear),
        ],
        vec![
            ShaderPass {
                kernel: reduce,
                bindings: vec![(0, 4), (1, 0), (2, 2), (4, 6)],
                dispatch: ShaderDispatch::Region {
                    extent: low,
                    outset: 2 * reach + 4 * factor,
                },
            },
            ShaderPass {
                kernel: horizontal,
                bindings: vec![(0, 4), (1, 2), (2, 3), (4, 7)],
                dispatch: ShaderDispatch::Region {
                    extent: low,
                    outset: reach + 2 * factor,
                },
            },
            ShaderPass {
                kernel: vertical,
                bindings: vec![(0, 4), (1, 3), (2, 2), (4, 7)],
                dispatch: ShaderDispatch::Region {
                    extent: low,
                    outset: 2 * factor,
                },
            },
            ShaderPass {
                kernel: expand,
                bindings: vec![(0, 5), (1, 2), (3, 1), (4, 6), (5, 8), (6, 9)],
                dispatch: ShaderDispatch::Region {
                    extent: ShaderExtent::FULL,
                    outset: 0,
                },
            },
        ],
    )?
    .with_input_radius((2 * reach + 4 * factor) as f32)
}
