#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "color.metal"

kernel void filter_clear_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(config.clear_color),xy);
}
kernel void filter_copy_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(source.read(xy),xy);
}
kernel void filter_source_alpha_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(float4(0,0,0,source.read(xy).a),xy);
}
kernel void filter_offset_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    int2 input=int2(xy)-int2(config.offset_x,config.offset_y);
    target.write(filter_contains(config,input)?source.read(uint2(input)):float4(0),xy);
}
kernel void filter_tile_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint2 origin=uint2(max(float2(config.rect_x0,config.rect_y0),0.0f));
    uint2 extent=uint2(max(float2(config.rect_x1,config.rect_y1),0.0f))-origin;
    if(any(extent==0)) return;
    uint2 input=origin+(xy+extent-origin%extent)%extent;
    target.write(all(input<uint2(config.width,config.height))?source.read(input):float4(0),xy);
}
kernel void filter_drop_shadow_mask_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    float alpha=source.read(xy).a;
    int2 output=int2(xy)+int2(config.offset_x,config.offset_y);
    if(alpha!=0 && filter_contains(config,output)) target.write(float4(alpha),uint2(output));
}
kernel void filter_source_over_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    target.write(unpack_pixel(source_over(pack_pixel(target.read(xy)),pack_pixel(source.read(xy)))),xy);
}
kernel void filter_svg_mask_coverage_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint4 pixel=byte_channels(pack_pixel(source.read(xy)));
    uint coverage=pixel.a;
    if(config.mask_kind==1) {
        uint denominator=max(pixel.a,1u);
        uint3 straight=(pixel.rgb*255+denominator/2)/denominator;
        coverage=((2126*straight.r+7152*straight.g+722*straight.b)*pixel.a+1275000)/2550000;
    }
    target.write(float4(float(coverage)*(1.0f/255.0f)),xy);
}

kernel void filter_color_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(color_filter(pack_pixel(target.read(xy)),config.filter_kind,config.amount)),xy);
}
kernel void filter_color_matrix_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(filter_position(config,tiles,id,xy)) target.write(unpack_pixel(matrix_filter(config,pack_pixel(target.read(xy)))),xy);
}
