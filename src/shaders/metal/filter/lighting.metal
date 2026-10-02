#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "lighting_math.metal"
kernel void filter_lighting_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(lighting_pixel(config,source,xy)),xy);
}
