#![cfg(all(feature = "metal", target_os = "macos"))]
use tileink::*;

const SOURCE: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Params { float4 tint; uint4 indices; };
struct Images { array<texture2d<float>, 2> images [[id(0)]]; };
kernel void combine(constant Params& params [[buffer(0)]], texture2d_array<float> atlas [[texture(12)]], constant Images& table [[buffer(30)]], texture2d<float, access::write> output [[texture(3)]], uint2 p [[thread_position_in_grid]]) {
    if (p.x >= 3 || p.y >= 2) return;
    output.write((atlas.read(p, params.indices.x) + table.images[params.indices.y].read(p)) * params.tint, p);
}
"#;
fn pipeline() -> Result<ShaderPipeline, Box<dyn std::error::Error>> {
    pipeline_source(SOURCE)
}
fn pipeline_source(source: &str) -> Result<ShaderPipeline, Box<dyn std::error::Error>> {
    use ShaderBindingKind as B;
    use ShaderResourceKind as K;
    let bindings = [
        ("params", 0, B::Uniform(32)),
        ("atlas", 12, B::TextureArray),
        ("table", 30, B::TextureTable(2)),
        (
            "output",
            3,
            B::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
        ),
    ];
    let resources = [
        K::Buffer,
        K::TextureArray,
        K::TextureTable(2),
        K::Texture(ShaderTextureFormat::Rgba8Unorm),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, kind)| ShaderResource::External { index, kind })
    .collect();
    let kernel = ShaderKernel::new(
        ShaderSources {
            metal: Some(source.into()),
            hlsl: None,
        },
        "combine",
        [8, 8, 1],
        bindings
            .into_iter()
            .map(|(name, slot, kind)| ShaderBinding {
                name: name.into(),
                slot,
                kind,
            })
            .collect(),
    )?;
    Ok(ShaderPipeline::new(
        resources,
        vec![ShaderPass {
            kernel,
            bindings: vec![(0, 0), (12, 1), (30, 2), (3, 3)],
            dispatch: ShaderDispatch::Groups([1; 3]),
        }],
    )?)
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn public_native_invocations_support_uniforms_arrays_tables_and_foreign_handle_rejection()
-> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(
        NativeBackend::Metal,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let pipeline = pipeline()?;
    let image = context.create_texture(3, 2)?;
    for _ in 0..3 {
        let mut batch = NativeShaderBatch::new(&context);
        let mut words = vec![0.5f32.to_bits(); 4];
        words.extend([1, 1, 0, 0]);
        let params = batch.buffer(bytemuck::cast_slice(&words).to_vec())?;
        let mut pixels = vec![0; 24];
        pixels.extend([80, 100, 120, 255].repeat(6));
        let atlas = batch.texture_array_rgba8([3, 2, 2], pixels)?;
        let a = batch.texture_rgba8([3, 2], [20, 40, 60, 0].repeat(6))?;
        let b = batch.texture_rgba8([3, 2], [40, 60, 80, 0].repeat(6))?;
        let table = batch.texture_table(&[a, b])?;
        let output = batch.import_texture(&image)?;
        let mut foreign = NativeShaderBatch::new(&context);
        let invalid = foreign.buffer(vec![0; 32])?;
        assert!(
            batch
                .record(&pipeline, &[invalid, atlas, table, output])
                .is_err()
        );
        assert!(
            batch
                .record(&pipeline, &[params, a, table, output])
                .is_err()
        );
        let alias = batch.texture_table(&[a, output])?;
        assert!(
            batch
                .record(&pipeline, &[params, atlas, alias, output])
                .is_err()
        );
        batch.record(&pipeline, &[params, atlas, table, output])?;
        batch.submit()?.wait()?;
        assert_eq!(
            image.readback()?.readback()?.pixels,
            vec![u32::from_le_bytes([60, 80, 100, 128]); 6]
        );
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn public_native_invocations_reject_reflected_table_count_and_uniform_size_mismatches()
-> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(
        NativeBackend::Metal,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    for source in [
        SOURCE.replace("2> images", "3> images"),
        SOURCE.replace(
            "float4 tint; uint4 indices;",
            "float4 tint; uint4 indices; float4 extra;",
        ),
    ] {
        let pipeline = pipeline_source(&source)?;
        let mut batch = NativeShaderBatch::new(&context);
        let params = batch.buffer(vec![0; 32])?;
        let atlas = batch.texture_array_rgba8([3, 2, 2], vec![0; 48])?;
        let image = batch.texture_rgba8([3, 2], vec![0; 24])?;
        let table = batch.texture_table(&[image, image])?;
        let output = batch.texture_rgba8([3, 2], vec![0; 24])?;
        batch.record(&pipeline, &[params, atlas, table, output])?;
        assert!(
            batch.submit().is_err(),
            "reflected native ABI mismatch was accepted"
        );
    }
    context.check_validation()?;
    Ok(())
}

#[test]
fn every_precompiled_filter_is_describable_using_only_public_pipeline_types()
-> Result<(), Box<dyn std::error::Error>> {
    use ShaderBindingKind as B;
    use ShaderResourceKind as K;
    for artifact in NATIVE_SHADER_ARTIFACTS
        .iter()
        .filter(|a| a.entry.starts_with("filter_") || a.entry.starts_with("progressive_blur_"))
    {
        let kernel = ShaderKernel::precompiled(artifact)?;
        assert_eq!(kernel.entry(), artifact.entry);
        assert_eq!(kernel.workgroup(), artifact.workgroup);
        let resources = kernel
            .bindings()
            .iter()
            .enumerate()
            .map(|(index, b)| ShaderResource::External {
                index,
                kind: match b.kind {
                    B::Uniform(_) | B::Config | B::ReadBuffer | B::ReadWriteBuffer => K::Buffer,
                    B::Texture => K::Texture(ShaderTextureFormat::Rgba8Unorm),
                    B::WriteTexture(f) | B::ReadWriteTexture(f) => K::Texture(f),
                    B::TextureArray => K::TextureArray,
                    B::TextureTable(n) => K::TextureTable(n),
                    B::Sampler => K::Sampler,
                },
            })
            .collect();
        let bindings = kernel
            .bindings()
            .iter()
            .enumerate()
            .map(|(i, b)| (b.slot, i))
            .collect();
        let p = ShaderPipeline::new(
            resources,
            vec![ShaderPass {
                kernel,
                bindings,
                dispatch: ShaderDispatch::Groups([1; 3]),
            }],
        )?;
        assert_eq!(p.external_count(), p.resources().len());
        assert_eq!(p.passes().len(), 1);
    }
    Ok(())
}
