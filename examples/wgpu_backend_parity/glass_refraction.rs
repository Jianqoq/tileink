use super::{Result, gpu, pixels, probe};
use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{BlurSampling, Canvas, Filter, Radius, RectLiquidGlass, Region};

fn edge_scene() -> Canvas {
    // A refracted sample near a circle combines with a blurred rectangle at a
    // byte-rounding boundary. Both shapes are needed to expose the original bug.
    let mut scene = Canvas::new(1080, 560, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 1080.0, 560.0),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );
    scene.push_rect(
        Rect::new(496.0, 106.0, 550.0, 160.0),
        Radius::all(8.0),
        Color::from_rgb8(244, 63, 94),
    );
    scene.push_circle(
        Circle::new((562.0, 133.0), 11.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 16,
            blur_sampling: BlurSampling::downsampled(4),
            tint: Color::from_rgba8(255, 255, 255, 0),
            refraction_dispersion: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(400.0, 112.0, 680.0, 448.0), Radius::all(42.0)),
    );
    scene.pop_layer();
    scene
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn glass_edge_refraction_preserves_pixel_rounding() -> Result<()> {
    let instance = probe::instance()?;
    let scene = edge_scene();
    let mut reference = None;
    let mut identity = None;
    for portable in [false, true] {
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
            let mut route = gpu::create(&instance, backend, portable, identity.as_deref())?;
            identity = Some(route.identity.clone());
            route.renderer.set_clear_color(Color::WHITE);
            route.renderer.render(&scene);
            let image = route.renderer.image();
            if let Some(reference) = &reference {
                let diff = pixels::compare(reference, &image)?;
                assert_eq!(diff.pixels, 0, "{}: {diff:?}", route.name);
            } else {
                reference = Some(image);
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn glass_refraction_geometry_matches_across_apis() -> Result<()> {
    // Exercise the production function, including the exact inputs captured at
    // the first divergent stage of the pixel regression above.
    let effects = include_str!("../../src/wgpu/shaders/filter/effects.wgsl");
    let start = effects
        .find("fn liquid_glass_edge(")
        .ok_or("missing production refraction function")?;
    let end = effects[start..]
        .find("\nfn liquid_glass_fresnel(")
        .ok_or("missing refraction boundary")?
        + start;
    let header = include_str!("../../src/wgpu/shaders/filter/header.wgsl");
    let declarations: Vec<_> = header
        .lines()
        .filter(|line| line.starts_with("const LIQUID_GLASS_EPSILON:"))
        .collect();
    assert_eq!(declarations.len(), 1, "one production refraction epsilon");
    let declaration = declarations[0];
    let epsilon: f32 = declaration
        .split_once('=')
        .ok_or("missing epsilon initializer")?
        .1
        .trim()
        .trim_end_matches(';')
        .parse()?;
    assert!(epsilon.is_finite() && epsilon > 0.0);
    let source = format!(
        "{declaration}\n{}\n{}",
        &effects[start..end],
        r#"
@group(0) @binding(0) var<storage,read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> output: array<u32>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = input[gid.x];
    output[gid.x] = bitcast<u32>(liquid_glass_edge(p.x,p.y,p.z));
}"#
    );
    let mut inputs = vec![[0.5_f32, 20.0, 1.4, 0.0]];
    for inside in [-0.5, 0.0, 0.5, 20.0] {
        inputs.push([inside, 20.0, f32::MAX, 0.0]);
    }
    for thickness in [0.0_f32, epsilon, 1.0, 20.0, 200.0] {
        for fraction in [-0.25_f32, 0.0, 0.0001, 0.025, 0.25, 0.5, 0.9999, 1.0, 1.25] {
            for factor in [0.0_f32, 1.0, 1.0000001, 1.01, 1.4, 2.0, 10.0, 1000.0] {
                inputs.push([fraction * thickness, thickness, factor, 0.0]);
            }
        }
    }
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
            inputs.len(),
            [inputs.len() as u32, 1, 1],
        )?;
        for (input, word) in inputs.iter().zip(&words) {
            let actual = f32::from_bits(*word);
            assert!(
                actual.is_finite() && actual >= 0.0,
                "{backend:?}: {input:?} -> {actual}"
            );
            assert_refraction_reference(*input, actual, epsilon);
            if input[0] >= input[1].max(epsilon) || input[2] <= 1.0 {
                assert_eq!(actual, 0.0, "{backend:?}: {input:?}");
            }
        }
        if let Some(reference) = &reference {
            probe::assert_words_equal(reference, &words, "glass refraction geometry");
        } else {
            reference = Some(words);
        }
    }
    Ok(())
}

fn assert_refraction_reference(input: [f32; 4], actual: f32, epsilon: f32) {
    let [inside, thickness, factor, _] = input.map(f64::from);
    let thickness = thickness.max(f64::from(epsilon));
    let factor = factor.max(1.0);
    if inside >= thickness || factor == 1.0 {
        assert_eq!(actual, 0.0);
        return;
    }
    let ratio = (1.0 - inside / thickness).clamp(0.0, 1.0);
    let sine = ratio * ratio;
    // The f64 oracle independently evaluates Snell's law with trigonometry.
    // Bound the incident-sine error from f32 division/subtraction/squaring.
    // Near grazing incidence its conditioning amplifies tiny input errors, so
    // use the corresponding reference interval, not one loose global tolerance.
    let input_error = if inside <= 0.0 {
        0.0
    } else {
        4.0 * f64::from(f32::EPSILON)
    };
    let reference = |sine: f64| {
        if sine == 1.0 {
            // The analytic grazing limit avoids f64 angle subtraction losing
            // asin(1/factor) when the finite refractive index is very large.
            (factor * factor - 1.0).sqrt()
        } else {
            (sine.asin() - (sine / factor).asin()).tan().max(0.0)
        }
    };
    let lower = reference((sine - input_error).max(0.0));
    let upper = reference((sine + input_error).min(1.0));
    let arithmetic_error = 8.0 * f64::from(f32::EPSILON) * upper.max(1.0);
    let value = f64::from(actual);
    assert!(
        value >= lower - arithmetic_error && value <= upper + arithmetic_error,
        "{input:?}: refraction {actual}, independent reference interval {lower}..{upper}"
    );
}
