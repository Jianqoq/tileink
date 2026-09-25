#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "../shared/sdf/base.metal"
#include "sample.metal"
#include "glass/color.metal"
#include "glass/geometry.metal"
#include "glass/sample.metal"
#include "glass/pixel.metal"
kernel void filter_liquid_glass_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],
    texture2d<float,access::read> auxiliary [[texture(2)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(glass_pixel(config,source,auxiliary,xy,glass_distance(config,float2(xy)+0.5f))),xy);
}
kernel void filter_liquid_glass_rect_composite_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],
    texture2d<float,access::read> auxiliary [[texture(2)]],texture2d<float,access::read_write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    float2 p=float2(xy)+0.5f;
    float distance=rect_sample(p,float4(config.rect_x0,config.rect_y0,config.rect_x1,config.rect_y1),
        float4(config.radius_top_left,config.radius_top_right,config.radius_bottom_left,config.radius_bottom_right)).distance;
    uint alpha=coverage_u8(0.5f-distance);if(!alpha) return;
    uint color=glass_pixel(config,source,auxiliary,xy,glass_distance(config,p));
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),scale_pixel(color,alpha))),xy);
}
