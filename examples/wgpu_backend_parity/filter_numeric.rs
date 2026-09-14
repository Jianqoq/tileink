use super::{Result, common, gpu, pixels, probe, svg};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn filter_rounding_regressions_match_all_routes() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests");
    // These original fixtures exposed recurrence, normal, dot-product and noise
    // interpolation rounding defects. Exercise the actual filter bindings and
    // composition, including both texture execution paths, with no pixel tolerance.
    let names = [
        "filters/feDropShadow/only-stdDeviation.svg",
        "filters/feSpecularLighting/lighting-color=hsla.svg",
        "filters/feSpecularLighting/with-fePointLight.svg",
        "filters/feSpecularLighting/with-feSpotLight.svg",
        "filters/feSpecularLighting/with-feSpotLight-and-specular-and-exponent.svg",
        "filters/feTurbulence/baseFrequency=0.05-0.01.svg",
        "filters/feTurbulence/numOctaves=5.svg",
        "painting/stroke/control-points-clamping-1.svg",
        "painting/marker/nested.svg",
    ];
    let corpus = svg::Corpus::load(&names.map(|name| root.join(name)))?;
    let instance = probe::instance()?;
    let mut routes = Vec::new();
    let mut identity = None;
    for portable in [false, true] {
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
            let route = gpu::create(&instance, backend, portable, identity.as_deref())?;
            identity = Some(route.identity.clone());
            routes.push(route);
        }
    }
    for (name, tree) in names.iter().zip(&corpus.trees) {
        let (canvas, _, _) = common::svg_tree_to_scene(tree, 300)?;
        let mut reference = None;
        for route in &mut routes {
            route.renderer.render(&canvas);
            let actual = route.renderer.image();
            if let Some(reference) = &reference {
                let difference = pixels::compare(reference, &actual)?;
                assert_eq!(
                    difference.pixels, 0,
                    "{name}, {}: {difference:?}",
                    route.name
                );
            } else {
                reference = Some(actual);
            }
        }
    }
    corpus.verify_unchanged()?;
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn lighting_gradients_respect_region_edges_and_alpha_units() -> Result<()> {
    let effects = include_str!("../../src/wgpu/shaders/filter/effects.wgsl");
    let (_, gradients) = effects.split_once("fn alpha_gradient_x(").unwrap();
    let (gradients, _) = gradients.split_once("fn composite_inputs_pixel(").unwrap();
    let source = format!(
        "{}\nfn alpha_gradient_x({gradients}\n{}",
        r#"
struct Region { region_x0:u32, region_y0:u32, region_width:u32, region_height:u32 }
var<private> config: Region;
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
fn source_pixel_at(x:u32, y:u32) -> u32 { return input[4u + x + 9u * y] << 24u; }
"#,
        r#"
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    config = Region(input[0], input[1], input[2], input[3]);
    let x = id.x + config.region_x0;
    let y = id.y + config.region_y0;
    let weights_x = 2u + u32(id.x > 0u) + u32(id.x + 1u < config.region_width);
    let weights_y = 2u + u32(id.y > 0u) + u32(id.y + 1u < config.region_height);
    let edge_x = select(1.0, 2.0, id.x == 0u || id.x + 1u == config.region_width);
    let edge_y = select(1.0, 2.0, id.y == 0u || id.y + 1u == config.region_height);
    // Recover the signed integer Sobel sums to test units and edge semantics
    // independently of the lighting equation and final color quantization.
    let dx = i32(round(alpha_gradient_x(x, y) * 255.0 * f32(weights_y) / edge_x));
    let dy = i32(round(alpha_gradient_y(x, y) * 255.0 * f32(weights_x) / edge_y));
    let index = 2u * (id.x + config.region_width * id.y);
    output[index] = bitcast<u32>(dx);
    output[index + 1u] = bitcast<u32>(dy);
}
"#
    );
    let instance = probe::instance()?;
    let dx = gpu::create(&instance, wgpu::Backend::Dx12, false, None)?;
    let vk = gpu::create(&instance, wgpu::Backend::Vulkan, false, Some(&dx.identity))?;
    for (x0, y0) in [(0_u32, 0_u32), (2, 3)] {
        for (width, height) in [(1_u32, 1_u32), (1, 5), (5, 1), (2, 2), (3, 5), (5, 3)] {
            for pattern in 0..3 {
                let mut input = vec![x0, y0, width, height];
                input.extend((0_u32..81).map(|i| match pattern {
                    0 => 127,
                    1 => {
                        if (i % 9 + i / 9) % 2 == 0 {
                            0
                        } else {
                            255
                        }
                    }
                    _ => (i % 9 * 31 + i / 9 * 17) % 256,
                }));
                let alpha = |x: u32, y: u32| input[(4 + x + 9 * y) as usize] as i32;
                let mut expected = Vec::new();
                for y in y0..y0 + height {
                    for x in x0..x0 + width {
                        let mut gx = 0_i32;
                        let mut gy = 0_i32;
                        for offset in -1..=1_i32 {
                            let weight = if offset == 0 { 2 } else { 1 };
                            let sy = y as i32 + offset;
                            if sy >= y0 as i32 && sy < (y0 + height) as i32 {
                                gx += weight
                                    * (alpha((x + 1).min(x0 + width - 1), sy as u32)
                                        - alpha(x.saturating_sub(1).max(x0), sy as u32));
                            }
                            let sx = x as i32 + offset;
                            if sx >= x0 as i32 && sx < (x0 + width) as i32 {
                                gy += weight
                                    * (alpha(sx as u32, (y + 1).min(y0 + height - 1))
                                        - alpha(sx as u32, y.saturating_sub(1).max(y0)));
                            }
                        }
                        expected.extend([gx as u32, gy as u32]);
                    }
                }
                for route in [&dx, &vk] {
                    let actual = probe::words(
                        route.renderer.device(),
                        route.renderer.queue(),
                        &source,
                        &input,
                        expected.len(),
                        [width, height, 1],
                    )?;
                    probe::assert_words_equal(
                        &expected,
                        &actual,
                        &format!(
                            "{}: region ({x0},{y0}) {width}x{height}, pattern {pattern}",
                            route.name
                        ),
                    );
                }
            }
        }
    }
    Ok(())
}
