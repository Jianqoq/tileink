#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
kernel void filter_composite_surface_direct_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read_write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    int2 input=int2(xy)-int2(config.offset_x,config.offset_y);
    if(any(input<0) || any(input>=int2(config.kernel_columns,config.kernel_rows))) return;
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),pack_pixel(source.read(uint2(input))))),xy);
}
