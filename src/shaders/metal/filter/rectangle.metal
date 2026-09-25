#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "../shared/sdf/base.metal"
#include "sample.metal"
uint rectangle_alpha(constant FilterConfig& c,uint2 xy) {
    SdfSample sample=rect_sample(float2(xy)+0.5f,float4(c.rect_x0,c.rect_y0,c.rect_x1,c.rect_y1),
        float4(c.radius_top_left,c.radius_top_right,c.radius_bottom_left,c.radius_bottom_right));
    return coverage_u8(0.5f-sample.distance);
}
kernel void filter_rect_mask_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(float4(float(rectangle_alpha(config,xy))*(1.0f/255.0f)),xy);
}
kernel void filter_composite_direct_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read> mask [[texture(2)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint alpha=config.mask_enabled?pack_pixel(mask.read(xy))>>24:255;
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),scale_pixel(pack_pixel(source.read(xy)),alpha))),xy);
}
kernel void filter_composite_rect_direct_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read_write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint alpha=rectangle_alpha(config,xy);if(!alpha) return;
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),scale_pixel(pack_pixel(source.read(xy)),alpha))),xy);
}
kernel void filter_upsample_rect_composite_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read_write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint2 lower=uint2(config.source_x0,config.source_y0),upper=uint2(config.source_x1,config.source_y1);
    if(any(lower>=upper)) return;
    uint alpha=rectangle_alpha(config,xy);if(!alpha) return;
    float2 position=clamp((float2(xy)+0.5f)/float(max(config.downsample,1u))-0.5f,float2(lower),float2(upper-1));
    float4 sample=config.upsample_filter?sample_premul(source,uint2(config.width,config.height),position):source.read(uint2(rint(position)));
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),scale_pixel(pack_pixel(sample),alpha))),xy);
}
