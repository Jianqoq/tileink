#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "color_space.metal"
#include "../shared/blend/channels.metal"
#include "../shared/blend/compose.metal"

// SVG input compositing shares the renderer's premultiplied byte boundary.
kernel void filter_blend_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read> backdrop [[texture(2)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint s=pack_pixel(source.read(xy)),d=pack_pixel(backdrop.read(xy));
    if(config.linear_rgb==1) {s=filter_premul_srgb_to_linear(s);d=filter_premul_srgb_to_linear(d);}
    uint result=blend_pixel(d,s,config.blend_mode);
    target.write(unpack_pixel(config.linear_rgb==1?filter_premul_linear_to_srgb(result):result),xy);
}
kernel void filter_composite_inputs_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read> backdrop [[texture(2)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint s=pack_pixel(source.read(xy)),d=pack_pixel(backdrop.read(xy)),output;
    if(config.filter_kind==5) {
        float4 a=unpack_pixel(s),b=unpack_pixel(d),k=config.matrix_bias;
        output=pack_pixel(k.x*a*b+k.y*a+k.z*b+k.w);
    } else {
        uint compose=3;
        if(config.filter_kind==1) compose=5;
        else if(config.filter_kind==2) compose=7;
        else if(config.filter_kind==3) compose=9;
        else if(config.filter_kind==4) compose=11;
        output=blend_pixel(d,s,compose<<8);
    }
    target.write(unpack_pixel(output),xy);
}
kernel void filter_apply_region_mask(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> mask [[texture(2)]],texture2d<float,access::read_write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint alpha=mul255(pack_pixel(target.read(xy))>>24,pack_pixel(mask.read(xy))>>24);
    target.write(float4(float(alpha)*(1.0f/255.0f)),xy);
}
