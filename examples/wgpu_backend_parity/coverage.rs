use super::{Result, common, gpu, pixels};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn long_diagonal_stroke_has_exact_backend_coverage() -> Result<()> {
    let tree = usvg::Tree::from_str(
        r#"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
            <line x1="20" y1="40" x2="160" y2="180" stroke="green"/>
        </svg>"#,
        &usvg::Options::default(),
    )?;
    let (scene, _, _) = common::svg_tree_to_scene(&tree, 300)?;
    let instance = super::probe::instance()?;
    let mut luid = None;
    let mut images = Vec::new();
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let mut route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        route.renderer.render(&scene);
        images.push(route.renderer.image());
    }
    let difference = pixels::compare(&images[0], &images[1])?;
    assert_eq!(
        difference.pixels, 0,
        "diagonal coverage differs: {difference:?}"
    );
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn scan_intersections_agree_at_tile_boundaries() -> Result<()> {
    let instance = super::probe::instance()?;
    let mut input: Vec<[f32; 4]> = Vec::new();
    for (x0, y0, x1, y1) in [
        (30.53033, 59.46967, 240.53033, 269.46967),
        (29.46967, 60.53033, 239.46967, 270.53033),
        (150.0, 22.5, 225.0, 262.5),
        (225.0, 262.5, 30.0, 112.5),
    ] {
        for coordinate in (16..288).step_by(16) {
            input.push([x0, y0, x1, y1]);
            input.push([coordinate as f32, 0.0, 0.0, 0.0]);
        }
    }
    let emit = include_str!("../../src/wgpu/shaders/scan/emit.wgsl");
    let (_, functions) = emit.split_once("fn x_at_y(").unwrap();
    let source = format!(
        "const SCAN_EPSILON: f32 = 1.0e-6;\nconst TILE_CLIP_NUDGE: f32 = 1.0e-3;\nfn x_at_y({functions}\n{}",
        r#"@group(0) @binding(0) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = input[2u * id.x];
    let at = input[2u * id.x + 1u].x;
    output[2u * id.x] = bitcast<u32>(x_at_y(p.x, p.y, p.z, p.w, at));
    output[2u * id.x + 1u] = bitcast<u32>(y_at_x(p.x, p.y, p.z, p.w, at));
}"#,
    );
    let count = input.len() / 2;
    let mut luid = None;
    let mut outputs = Vec::new();
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        outputs.push(super::probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &source,
            bytemuck::cast_slice(&input),
            2 * count,
            [count as u32, 1, 1],
        )?);
    }
    super::probe::assert_words_equal(&outputs[0], &outputs[1], "scan intersection operations");
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn tile_fill_quantizes_identically_across_apis() -> Result<()> {
    let instance = super::probe::instance()?;
    // Captured scan segments retain the exact fractional coordinates at the two
    // regressions. Their analytic alpha values lie below 142.5 and 124.5.
    let cases = [
        (
            vec![
                [1078190592, 0, 1098907647, 1095698320, 1315859240],
                [1098907647, 1097922688, 1064335488, 0, 1315859240],
            ],
            2 * 16 + 3,
            142,
        ),
        (
            vec![[1093157568, 1098907648, 1093150976, 0, 1315859240]],
            4 * 16 + 10,
            124,
        ),
    ];
    let source = format!(
        "{}\n{}\n{}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"struct LineSegment { p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32 }
@group(0) @binding(0) var<storage, read> segments: array<LineSegment>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;"#,
        include_str!("../../src/wgpu/shaders/shared/coverage.wgsl"),
        r#"@compute @workgroup_size(16, 16) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x + 16u * id.y] = fill_alpha_at(0, 0u, 0u, arrayLength(&segments), id.x, id.y);
}"#,
    );
    let dx = gpu::create(&instance, wgpu::Backend::Dx12, false, None)?;
    let vk = gpu::create(&instance, wgpu::Backend::Vulkan, false, Some(&dx.identity))?;
    for (mut input, index, alpha) in cases {
        let mut reference: Option<Vec<u32>> = None;
        for _ in 0..2 {
            for route in [&dx, &vk] {
                let output = super::probe::words(
                    route.renderer.device(),
                    route.renderer.queue(),
                    &source,
                    bytemuck::cast_slice(&input),
                    256,
                    [1, 1, 1],
                )?;
                assert_eq!(
                    output[index], alpha,
                    "{}: analytic boundary coverage",
                    route.name
                );
                if let Some(reference) = &reference {
                    super::probe::assert_words_equal(
                        reference,
                        &output,
                        "tile coverage and reversed winding order",
                    );
                } else {
                    reference = Some(output);
                }
            }
            input.reverse();
        }
    }
    Ok(())
}
