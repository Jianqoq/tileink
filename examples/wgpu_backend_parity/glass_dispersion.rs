use super::{Result, gpu, probe};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn zero_dispersion_scale_preserves_sample_coordinates() -> Result<()> {
    let effects = include_str!("../../src/wgpu/shaders/filter/effects.wgsl");
    let function = |name: &str, next: &str| -> Result<&str> {
        let start = effects.find(name).ok_or("missing production function")?;
        let end = effects[start..]
            .find(next)
            .ok_or("missing production function boundary")?
            + start;
        Ok(&effects[start..end])
    };
    let header = include_str!("../../src/wgpu/shaders/filter/header.wgsl");
    let scale = header
        .lines()
        .find(|line| line.starts_with("const LIQUID_GLASS_REFRACTION_PIXEL_SCALE:"))
        .ok_or("missing production refraction scale")?;
    let pixel_scale: f32 = scale
        .split_once('=')
        .ok_or("invalid production refraction scale")?
        .1
        .trim()
        .trim_end_matches(';')
        .parse()?;
    let source = format!(
        "{scale}\n{}\n{}\n{}",
        function(
            "fn liquid_glass_dispersion_channel(",
            "\nfn liquid_glass_sample_alpha("
        )?,
        function("fn lerp_f32(", "\nfn lerp_vec4(")?,
        r#"
struct ProbeConfig { liquid_refraction_dispersion: f32 }
var<private> config: ProbeConfig;
var<private> sampled: vec2<f32>;
// Observe the actual coordinates at the production sampler boundary, before
// its texture access. The shader under test still computes the dispersion.
fn liquid_glass_sample_straight_channel(kind: u32, x: f32, y: f32, channel: u32) -> f32 {
    sampled = vec2<f32>(x, y);
    return 0.5;
}
@group(0) @binding(0) var<storage,read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> output: array<u32>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = input[gid.x];
    config.liquid_refraction_dispersion = p.z;
    let offset_x = p.x * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
    let offset_y = p.y * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
    let value = liquid_glass_dispersion_channel(552.0,112.0,offset_x,offset_y,p.w,2u,1.0);
    let index = gid.x * 4u;
    output[index] = bitcast<u32>(sampled.x);
    output[index + 1u] = bitcast<u32>(sampled.y);
    output[index + 2u] = bitcast<u32>(clamp(sampled.x,0.0,1079.0));
    output[index + 3u] = bitcast<u32>(clamp(sampled.y,0.0,559.0));
}"#
    );
    // The first two controls have an exactly zero channel scale. The remaining
    // extreme cases use production red/blue factors near cancellation.
    let inputs = [
        [f32::MAX, 0.0, 1.0, 2.0],
        [0.0, f32::MAX, 1.0, 2.0],
        [f32::MAX, 0.0, 50.000_05, 1.02],
        [0.0, f32::MAX, 50.000_05, 1.02],
        [f32::MAX, 0.0, -50.000_05, 0.98],
        [0.0, f32::MAX, -50.000_05, 0.98],
        [0.25, -0.5, 7.0, 1.02],
        [-0.5, 0.25, 7.0, 0.98],
    ];
    let instance = probe::instance()?;
    let mut identity = None;
    let mut reference: Option<Vec<u32>> = None;
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, identity.as_deref())?;
        identity = Some(route.identity.clone());
        let words = probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &source,
            bytemuck::cast_slice(&inputs),
            inputs.len() * 4,
            [inputs.len() as u32, 1, 1],
        )?;
        let mut clamped = Vec::new();
        for (index, values) in words.chunks_exact(4).enumerate() {
            let values: Vec<_> = values.iter().map(|word| f32::from_bits(*word)).collect();
            assert!(
                !values[0].is_nan() && !values[1].is_nan(),
                "{backend:?}: case {index} produced NaN sample coordinates: {values:?}"
            );
            assert!(
                values[2].is_finite() && values[3].is_finite(),
                "{backend:?}: case {index} produced nonfinite clamped coordinates"
            );
            if index < 2 {
                assert_eq!(
                    &values[..2],
                    &[552.0, 112.0],
                    "zero chromatic scale must preserve the sample position"
                );
            }
            if index >= 6 {
                let [x, y, dispersion, chromatic] = inputs[index].map(f64::from);
                let factor = 1.0 - (chromatic - 1.0) * dispersion;
                // Independent double-precision geometry checks retain normal
                // displacement; cross-API equality alone would accept zero offsets.
                for (actual, expected) in values[..2].iter().zip([
                    552.0 + x * f64::from(pixel_scale) * factor,
                    112.0 + y * f64::from(pixel_scale) * factor,
                ]) {
                    let bound = 4.0 * f64::from(f32::EPSILON) * expected.abs().max(1.0);
                    assert!(
                        (f64::from(*actual) - expected).abs() <= bound,
                        "{backend:?}: case {index} lost its nonzero dispersion: {actual} != {expected}"
                    );
                }
            }
            clamped.extend_from_slice(&words[index * 4 + 2..index * 4 + 4]);
        }
        if let Some(reference) = &reference {
            probe::assert_words_equal(reference, &clamped, "dispersed sample coordinates");
        } else {
            reference = Some(clamped);
        }
    }
    Ok(())
}
