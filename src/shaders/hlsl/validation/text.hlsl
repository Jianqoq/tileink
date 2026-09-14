#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../fine/text/basic.hlsli"
#include "../fine/text/auto.hlsli"
ByteAddressBuffer requests : register(t9);
RWByteAddressBuffer output : register(u10);
struct TextRequestConfig { uint count; uint pad0; uint pad1; uint pad2; };
ConstantBuffer<TextRequestConfig> request_config : register(b11);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void text_words(uint3 id : SV_DispatchThreadID) {
    // Explicit logical length also excludes storage padding for an empty request.
    if (id.x >= request_config.count) return;
    uint4 v=requests.Load4(id.x*16u);
    uint coverage=combine_alpha(v.z & 255u,v.w);
    uint base=id.x*20u;
    output.Store(base,src_over_subpixel_mask_u8(v.x,v.y,v.z,v.w));
    output.Store(base+4u,src_over_mask_linear_u8(v.x,v.y,coverage));
    output.Store(base+8u,src_over_mask_linear_auto_u8(v.x,v.y,coverage));
    output.Store(base+12u,src_over_subpixel_mask_linear_u8(v.x,v.y,v.z,v.w));
    output.Store(base+16u,src_over_subpixel_mask_linear_auto_u8(v.x,v.y,v.z,v.w));
}
