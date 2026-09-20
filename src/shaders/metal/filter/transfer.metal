#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"

kernel void filter_component_transfer_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tables [[buffer(7)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint4 pixel=byte_channels(pack_pixel(target.read(xy)));
    uint3 straight=pixel.a?min((pixel.rgb*255+pixel.a/2)/pixel.a,255u):uint3(0);
    const device uint* table=tables+config.table_index*1024;
    float4 mapped=float4(table[straight.r],table[256+straight.g],table[512+straight.b],table[768+pixel.a])*(1.0f/255.0f);
    target.write(unpack_pixel(pack_pixel(float4(mapped.rgb*mapped.a,mapped.a))),xy);
}
