#include "../constants.hlsli"
#include "../shared/pixel.hlsli"

// Validation adapter calls the same helpers used by the fine interpreter.
struct MathConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<MathConfig> config : register(b0);
ByteAddressBuffer source : register(t1);
RWByteAddressBuffer destination : register(u2);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void pixel_math_words(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.count) return;
    uint input_base=id.x*32u;
    uint output_base=id.x*56u;
    uint a=source.Load(input_base), b=source.Load(input_base+4u);
    uint src=source.Load(input_base+8u), dst=source.Load(input_base+12u);
    float c=asfloat(source.Load(input_base+16u)), t=asfloat(source.Load(input_base+20u));
    uint rule=source.Load(input_base+24u);
    float4 pixel=rgba8_to_unorm(src);
    destination.Store(output_base+0u,mul_div255(a,b));
    destination.Store(output_base+4u,combine_alpha(a,b));
    destination.Store(output_base+8u,scale_premul_u8(src,b));
    destination.Store(output_base+12u,src_over_premul_u8(dst,src));
    destination.Store(output_base+16u,src);
    destination.Store(output_base+20u,coverage_to_u8(c));
    destination.Store(output_base+24u,coverage_to_alpha(c,rule));
    destination.Store(output_base+28u,lerp_premul_u8(src,dst,t));
    destination.Store(output_base+32u,unorm_to_rgba8(src_over_premul_unorm(rgba8_to_unorm(dst),rgba8_to_unorm(src))));
    destination.Store(output_base+36u,unorm_to_rgba8(scale_premul_u8_to_unorm(src,b)));
    destination.Store(output_base+40u,coverage_to_u8(straight_channel(a,b)));
    destination.Store(output_base+44u,pack_premul_rgba8(pixel.r,pixel.g,pixel.b,pixel.a));
    destination.Store(output_base+48u,coverage_to_u8(rem_euclid_f32(c,3.0)));
    destination.Store(output_base+52u,asuint(signum_f32(c)));
}
