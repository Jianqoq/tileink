use super::*;

const SOURCE: &str = r#"
#include <metal_stdlib>
using namespace metal;
kernel void sample_color(texture2d<float> source [[texture(0)]], sampler filter [[sampler(2)]], texture2d<float, access::write> destination [[texture(1)]], uint2 pixel [[thread_position_in_grid]]) {
    if (any(pixel != uint2(0))) return;
    destination.write(source.sample(filter, float2(0.5f)), pixel);
}
"#;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn public_native_samplers_preserve_filtering_and_reject_foreign_handles()
-> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(
        NativeBackend::Metal,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let kernel = ShaderKernel::new(
        ShaderSources {
            metal: Some(SOURCE.into()),
            hlsl: None,
        },
        "sample_color",
        [1; 3],
        vec![
            ShaderBinding {
                name: "source".into(),
                slot: 0,
                kind: ShaderBindingKind::Texture,
            },
            ShaderBinding {
                name: "filter".into(),
                slot: 2,
                kind: ShaderBindingKind::Sampler,
            },
            ShaderBinding {
                name: "destination".into(),
                slot: 1,
                kind: ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            },
        ],
    )?;
    let pipeline = ShaderPipeline::new(
        vec![
            ShaderResource::External {
                index: 0,
                kind: ShaderResourceKind::Texture(ShaderTextureFormat::Rgba8Unorm),
            },
            ShaderResource::External {
                index: 1,
                kind: ShaderResourceKind::Sampler,
            },
            ShaderResource::External {
                index: 2,
                kind: ShaderResourceKind::Texture(ShaderTextureFormat::Rgba8Unorm),
            },
        ],
        vec![ShaderPass {
            kernel,
            bindings: vec![(0, 0), (2, 1), (1, 2)],
            dispatch: ShaderDispatch::Groups([1; 3]),
        }],
    )?;
    let image = context.create_texture(1, 1)?;
    for (filter, expected) in [
        (ShaderSampler::Nearest, [0, 0, 255, 255]),
        (ShaderSampler::Linear, [128, 0, 128, 255]),
    ] {
        let mut batch = NativeShaderBatch::new(&context);
        let source = batch.texture_rgba8([2, 1], vec![255, 0, 0, 255, 0, 0, 255, 255])?;
        let sampler = batch.sampler(filter);
        let output = batch.import_texture(&image)?;
        let mut foreign = NativeShaderBatch::new(&context);
        let invalid = foreign.sampler(filter);
        assert!(batch.record(&pipeline, &[source, invalid, output]).is_err());
        batch.record(&pipeline, &[source, sampler, output])?;
        batch.submit()?.wait()?;
        assert_eq!(
            image.readback()?.readback()?.pixels,
            [u32::from_le_bytes(expected)]
        );
    }
    context.check_validation()?;
    Ok(())
}
