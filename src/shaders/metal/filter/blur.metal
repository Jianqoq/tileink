#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "blur_math.metal"
kernel void filter_blur_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    float deviation=max(config.amount,0.0f);
    uint pixel=deviation>0?blur_pixel(config,source,xy,deviation):pack_pixel(source.read(xy));
    target.write(unpack_pixel(pixel),xy);
}
