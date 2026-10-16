#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "color_space.metal"

kernel void filter_component_transfer_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tables [[buffer(7)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint value=pack_pixel(target.read(xy));
    if(config.linear_rgb==1) value=filter_premul_srgb_to_linear(value);
    uint4 pixel=byte_channels(value);
    uint3 straight=pixel.a?min((pixel.rgb*255+pixel.a/2)/pixel.a,255u):uint3(0);
    const device uint* table=tables+config.table_index*1024;
    float4 mapped=float4(table[straight.r],table[256+straight.g],table[512+straight.b],table[768+pixel.a])*(1.0f/255.0f);
    uint result=pack_pixel(float4(mapped.rgb*mapped.a,mapped.a));
    target.write(unpack_pixel(config.linear_rgb==1?filter_premul_linear_to_srgb(result):result),xy);
}
