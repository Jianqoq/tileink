use super::{Result, gpu, probe};

// Captured white-to-black ramp shared by the two affine regressions. Preserve
// its original byte values so the fixture is independent of production ramp building.
const GRAYSCALE_RAMP: [u32; 64] = [
    4294967295, 4294704123, 4294440951, 4294177779, 4293914607, 4293651435, 4293388263, 4293125091,
    4292861919, 4292598747, 4292335575, 4292006610, 4291743438, 4291480266, 4291217094, 4290953922,
    4290690750, 4290427578, 4290164406, 4289901234, 4289638062, 4289374890, 4289111718, 4288848546,
    4288585374, 4288322202, 4288059030, 4287795858, 4287532686, 4287269514, 4287006342, 4286743170,
    4286414205, 4286151033, 4285887861, 4285624689, 4285361517, 4285098345, 4284835173, 4284572001,
    4284308829, 4284045657, 4283782485, 4283519313, 4283256141, 4282992969, 4282729797, 4282466625,
    4282203453, 4281940281, 4281677109, 4281413937, 4281150765, 4280821800, 4280558628, 4280295456,
    4280032284, 4279769112, 4279505940, 4279242768, 4278979596, 4278716424, 4278453252, 4278190080,
];

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn gradient_interpolation_rounds_exact_eighths() -> Result<()> {
    let instance = probe::instance()?;
    let mut input: Vec<[u32; 4]> = Vec::new();
    let mut expected = Vec::new();
    for left in 0..=255_u32 {
        for right in 0..=255_u32 {
            for eighths in [0, 1, 4, 5, 7, 8] {
                input.push([left * 0x01010101, right * 0x01010101, eighths, 0]);
                expected.push(((left * (8 - eighths) + right * eighths + 4) / 8) * 0x01010101);
            }
        }
    }
    let source = format!(
        "{}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"
@group(0) @binding(0) var<storage, read> input: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= arrayLength(&input)) { return; }
    let p = input[id.x];
    output[id.x] = lerp_premul_u8(p.x, p.y, f32(p.z) * 0.125);
}"#
    );
    let mut luid = None;
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        let actual = probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &source,
            bytemuck::cast_slice(&input),
            expected.len(),
            [expected.len().div_ceil(64) as u32, 1, 1],
        )?;
        probe::assert_words_equal(
            &expected,
            &actual,
            &format!("{backend:?}: 8-bit interpolation"),
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn linear_gradient_affine_rounding_is_pixel_exact() -> Result<()> {
    let instance = probe::instance()?;
    let mut input = vec![
        2, 0, 21, 64, 0, 0, 0, 255, 0, 1045220557, 1036831949, 1061997773, 1060320051, 998803593,
        2147483648, 2147483648, 998803593, 3187671040, 3187671040, 0, 0,
    ];
    input.extend_from_slice(&GRAYSCALE_RAMP);
    let brush = include_str!("../../src/wgpu/shaders/shared/brush.wgsl");
    let (_, linear) = brush.split_once("if (kind == GPU_BRUSH_LINEAR) {").unwrap();
    let (linear, _) = linear
        .split_once("} else if (kind == GPU_BRUSH_RADIAL)")
        .unwrap();
    let (_, ramp) = brush.split_once("fn sample_ramp(").unwrap();
    let source = format!(
        "{}\nconst GPU_EXTEND_REPEAT: u32 = 1u;\nconst GPU_EXTEND_REFLECT: u32 = 2u;\n{}\nfn sample_ramp({ramp}\nfn sample_linear(x: f32, y: f32) -> u32 {{\nlet base = 9u; let payload_offset = 21u; let payload_len = 64u; let extend = 0u; var color = 0u;\n{linear}\nreturn color;\n}}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
fn brush_word(index: u32) -> u32 { return input[index]; }
fn brush_param(base: u32, index: u32) -> f32 { return bitcast<f32>(input[base + index]); }"#,
        r#"@compute @workgroup_size(16, 16) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x + 240u * id.y] = sample_linear(f32(id.x) + 30.5, f32(id.y) + 30.5);
}"#,
    );
    let mut luid = None;
    let mut outputs = Vec::new();
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        outputs.push(probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &source,
            &input,
            240 * 240,
            [15, 15, 1],
        )?);
    }
    probe::assert_words_equal(&outputs[0], &outputs[1], "linear gradient pixels");
    Ok(())
}

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn radial_gradient_affine_rounding_is_pixel_exact() -> Result<()> {
    let instance = probe::instance()?;
    let mut input = vec![
        3, 0, 21, 64, 0, 0, 0, 255, 0, 1056964608, 1056964608, 1056964608, 1056964608, 0,
        1056964608, 996965265, 3137898633, 990414985, 996965265, 3190741484, 3174786990,
    ];
    input.extend_from_slice(&GRAYSCALE_RAMP);
    let brush = include_str!("../../src/wgpu/shaders/shared/brush.wgsl");
    let (_, radial) = brush.split_once("fn sample_radial(").unwrap();
    let (radial, _) = radial.split_once("fn sample_four_corner(").unwrap();
    let (_, ramp) = brush.split_once("fn sample_ramp(").unwrap();
    let source = format!(
        "{}\nconst GPU_EXTEND_REPEAT: u32 = 1u;\nconst GPU_EXTEND_REFLECT: u32 = 2u;\n{}\nfn sample_ramp({ramp}\nfn sample_radial({radial}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
fn brush_word(index: u32) -> u32 { return input[index]; }
fn brush_param(base: u32, index: u32) -> f32 { return bitcast<f32>(input[base + index]); }"#,
        r#"@compute @workgroup_size(16, 16) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x + 240u * id.y] = sample_radial(f32(id.x) + 30.5, f32(id.y) + 30.5, 9u, 0u, 21u, 64u);
}"#,
    );
    let mut luid = None;
    let mut outputs = Vec::new();
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let route = gpu::create(&instance, backend, false, luid.as_deref())?;
        luid = Some(route.identity.clone());
        outputs.push(probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &source,
            &input,
            240 * 240,
            [15, 15, 1],
        )?);
    }
    probe::assert_words_equal(&outputs[0], &outputs[1], "radial gradient pixels");
    Ok(())
}
