// Test-only adapter around production shared/pixel.wgsl; no copied math.
struct MathConfig { count:u32, pad0:u32, pad1:u32, pad2:u32 }
@group(0) @binding(0) var<uniform> config:MathConfig;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<storage,read_write> destination:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn pixel_math_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x>=config.count) { return; }
    let input_base=id.x*8u;
    let output_base=id.x*14u;
    let a=source[input_base]; let b=source[input_base+1u];
    let src=source[input_base+2u]; let dst=source[input_base+3u];
    let c=bitcast<f32>(source[input_base+4u]); let t=bitcast<f32>(source[input_base+5u]);
    let rule=source[input_base+6u]; let pixel=rgba8_to_unorm(src);
    destination[output_base+0u]=mul_div255(a,b);
    destination[output_base+1u]=combine_alpha(a,b);
    destination[output_base+2u]=scale_premul_u8(src,b);
    destination[output_base+3u]=src_over_premul_u8(dst,src);
    destination[output_base+4u]=src;
    destination[output_base+5u]=coverage_to_u8(c);
    destination[output_base+6u]=coverage_to_alpha(c,rule);
    destination[output_base+7u]=lerp_premul_u8(src,dst,t);
    destination[output_base+8u]=unorm_to_rgba8(src_over_premul_unorm(rgba8_to_unorm(dst),rgba8_to_unorm(src)));
    destination[output_base+9u]=unorm_to_rgba8(scale_premul_u8_to_unorm(src,b));
    destination[output_base+10u]=coverage_to_u8(straight_channel(a,b));
    destination[output_base+11u]=pack_premul_rgba8(pixel.r,pixel.g,pixel.b,pixel.a);
    destination[output_base+12u]=coverage_to_u8(rem_euclid_f32(c,3.0));
    destination[output_base+13u]=bitcast<u32>(signum_f32(c));
}
