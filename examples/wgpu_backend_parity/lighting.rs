use super::{Result, gpu, probe};

// Original radial SourceAlpha brush: its captured bits make this fast numeric
// regression independent of SVG parsing and production brush construction.
const RADIAL_ALPHA_BRUSH: [u32; 85] = [
    3, 0, 21, 64, 0, 0, 0, 255, 0, 1056964608, 1056964608, 1056964608, 1056964608, 0, 1056964608,
    998803593, 2147483648, 2147483648, 998803593, 3187671040, 3187671040, 4294967295, 4294967295,
    4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295,
    4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295,
    4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295,
    4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4294967295, 4227595259, 4092851187,
    3958107115, 3823363043, 3688618971, 3537031890, 3402287818, 3267543746, 3132799674, 2998055602,
    2863311530, 2728567458, 2593823386, 2459079314, 2324335242, 2189591170, 2038004089, 1903260017,
    1768515945, 1633771873, 1499027801, 1364283729, 1229539657, 1094795585, 960051513, 825307441,
    673720360, 538976288, 404232216, 269488144, 134744072, 0,
];

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn lighting_with_runtime_parameters_is_pixel_exact() -> Result<()> {
    let brush_input = RADIAL_ALPHA_BRUSH;
    let brush = include_str!("../../src/wgpu/shaders/shared/brush.wgsl");
    let (_, radial) = brush.split_once("fn sample_radial(").unwrap();
    let (radial, _) = radial.split_once("fn sample_four_corner(").unwrap();
    let (_, ramp) = brush.split_once("fn sample_ramp(").unwrap();
    let gradient = format!(
        "{}\n{}\nfn sample_ramp({ramp}\nfn sample_radial({radial}\n{}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"const GPU_EXTEND_REPEAT:u32=1u;const GPU_EXTEND_REFLECT:u32=2u;
@group(0) @binding(0) var<storage,read> input:array<u32>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
fn brush_word(i:u32)->u32 {return input[i];}
fn brush_param(base:u32,i:u32)->f32 {return bitcast<f32>(input[base+i]);}"#,
        r#"@compute @workgroup_size(16,16) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if(id.x>=300u || id.y>=300u) {return;}
    if(id.x>=30u && id.y>=30u && id.x<270u && id.y<270u) {output[id.x+id.y*300u]=sample_radial(f32(id.x)+0.5,f32(id.y)+0.5,9u,0u,21u,64u);}
}"#
    );
    let effects = include_str!("../../src/wgpu/shaders/filter/effects.wgsl");
    let (_, alpha) = effects.split_once("fn source_alpha_at(").unwrap();
    let (alpha, _) = alpha.split_once("fn filter_displacement_channel(").unwrap();
    let (_, gradients) = effects.split_once("fn alpha_gradient_x(").unwrap();
    let (gradients, _) = gradients.split_once("fn composite_inputs_pixel(").unwrap();
    let kernels = include_str!("../../src/wgpu/shaders/filter/kernels_effects.wgsl");
    let (_, kernel) = kernels.split_once("fn filter_lighting_region(").unwrap();
    let (kernel, _) = kernel.split_once("@compute").unwrap();
    let init = r#"config.lighting_output_kind = input[0];
config.surface_scale = bitcast<f32>(input[1]);
config.surface_origin_x = bitcast<i32>(input[2]);
config.surface_origin_y = bitcast<i32>(input[3]);
config.region_x0 = input[4];
config.region_y0 = input[5];
config.region_width = input[6];
config.region_height = input[7];
config.light_kind = input[8];
config.light_constant = bitcast<f32>(input[9]);
config.specular_exponent = bitcast<f32>(input[10]);
config.light_r = bitcast<f32>(input[11]);
config.light_g = bitcast<f32>(input[12]);
config.light_b = bitcast<f32>(input[13]);
config.light_p0 = bitcast<f32>(input[14]);
config.light_p1 = bitcast<f32>(input[15]);
config.light_p2 = bitcast<f32>(input[16]);
config.light_p3 = bitcast<f32>(input[17]);
config.light_p4 = bitcast<f32>(input[18]);
config.light_p5 = bitcast<f32>(input[19]);
config.light_p6 = bitcast<f32>(input[20]);
config.light_p7 = bitcast<f32>(input[21]);
config.light_p8 = bitcast<f32>(input[22]);
let region_ix = filter_region_index(gid);"#;
    let kernel = kernel.replace("let region_ix = filter_region_index(gid);", init);
    let source = format!(
        "{}\n{}\nfn source_alpha_at({alpha}\nfn alpha_gradient_x({gradients}\n@compute @workgroup_size(256) fn main({kernel}",
        include_str!("../../src/wgpu/shaders/shared/pixel.wgsl"),
        r#"struct LightConfig {
lighting_output_kind:u32,surface_scale:f32,surface_origin_x:i32,surface_origin_y:i32,
region_x0:u32,region_y0:u32,region_width:u32,region_height:u32,
light_kind:u32,light_constant:f32,specular_exponent:f32,light_r:f32,light_g:f32,light_b:f32,
light_p0:f32,light_p1:f32,light_p2:f32,light_p3:f32,light_p4:f32,light_p5:f32,light_p6:f32,light_p7:f32,light_p8:f32,
}
var<private> config:LightConfig;
@group(0) @binding(0) var<storage,read> input:array<u32>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
fn source_pixel_at(x:u32,y:u32)->u32 {return input[32u+x+y*300u];}
fn filter_region_index(id:vec3<u32>)->u32 {return id.x;}
fn filter_region_ix_valid(i:u32)->bool {let x=i%300u;let y=i/300u;return i<90000u && x>=6u && y>=6u && x<294u && y<294u;}
fn xy_for_region_ix(i:u32)->vec2<u32> {return vec2<u32>(i%300u,i/300u);}
fn target_ix_at(x:u32,y:u32)->u32 {return x+y*300u;}
fn target_store_ix(i:u32,p:u32) {output[i]=p;}
"#
    );
    let instance = probe::instance()?;
    let dx = gpu::create(&instance, wgpu::Backend::Dx12, false, None)?;
    let vk = gpu::create(&instance, wgpu::Backend::Vulkan, false, Some(&dx.identity))?;
    let mut base = Vec::new();
    for route in [&dx, &vk] {
        base.push(probe::words(
            route.renderer.device(),
            route.renderer.queue(),
            &gradient,
            &brush_input,
            90000,
            [19, 19, 1],
        )?);
    }
    assert_eq!(base[0], base[1], "lighting source gradient must agree");
    let mut input = vec![0; 32];
    input.extend_from_slice(&base[0]);
    // Runtime parameters matter: specializing them as shader constants hid a
    // normal/half-vector reassociation in the spotlight/exponent combination.
    for (mode, name) in [
        "green point",
        "seagreen point",
        "spotlight",
        "spotlight exponent",
    ]
    .into_iter()
    .enumerate()
    {
        input[..23].copy_from_slice(&[
            1,
            1.0_f32.to_bits(),
            0,
            0,
            6,
            6,
            288,
            288,
            1,
            8.0_f32.to_bits(),
            10.0_f32.to_bits(),
            (46.0_f32 / 255.0).to_bits(),
            (139.0_f32 / 255.0).to_bits(),
            (87.0_f32 / 255.0).to_bits(),
            100.0_f32.to_bits(),
            100.0_f32.to_bits(),
            10.0_f32.to_bits(),
            0,
            0,
            0,
            0,
            (-1.0_f32).to_bits(),
            0,
        ]);
        if mode == 0 {
            input[11] = 0;
            input[12] = (128.0_f32 / 255.0).to_bits();
            input[13] = 0;
        }
        if mode >= 2 {
            input[8] = 2;
            input[15] = 150.0_f32.to_bits();
            input[16] = 20.0_f32.to_bits();
            input[20] = 1.0_f32.to_bits();
            input[9] = 1.0_f32.to_bits();
            input[10] = 1.0_f32.to_bits();
        }
        if mode == 3 {
            input[9] = 5.0_f32.to_bits();
            input[10] = 10.0_f32.to_bits();
        }
        let a = probe::words(
            dx.renderer.device(),
            dx.renderer.queue(),
            &source,
            &input,
            90000,
            [90000u32.div_ceil(256), 1, 1],
        )?;
        let b = probe::words(
            vk.renderer.device(),
            vk.renderer.queue(),
            &source,
            &input,
            90000,
            [90000u32.div_ceil(256), 1, 1],
        )?;
        probe::assert_words_equal(&a, &b, name);
    }
    Ok(())
}
