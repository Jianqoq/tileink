#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "blur_math.metal"
// A 16x16 workgroup cooperatively loads the axis halo. Every participating lane
// reaches the barrier, including lanes outside the logical output rectangle.
kernel void filter_blur_shared_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint2 group [[threadgroup_position_in_grid]],uint2 local [[thread_position_in_threadgroup]]) {
    threadgroup uint pixels[768];
    uint2 origin=uint2(config.region_x0,config.region_y0)+group*16;
    if(config.compact_tiles) {
        uint index=group.x+group.y*config.dispatch_width;
        if(index>=config.active_tile_count) return;
        uint tile=tiles[index];origin=uint2(tile%config.tiles_width,tile/config.tiles_width)*16;
    }
    uint2 xy=origin+local;
    bool output=all(xy<uint2(config.width,config.height)) && filter_contains(config,int2(xy));
    float deviation=max(config.amount,0.0f);
    if(deviation<=0) {if(output) target.write(source.read(xy),xy);return;}
    uint radius=uint(max(ceil(deviation*3.0f),1.0f));
    if(radius>16) {if(output) target.write(unpack_pixel(blur_pixel(config,source,xy,deviation)),xy);return;}
    bool horizontal=config.blur_axis==0;
    uint stride=16+(horizontal?radius*2:0),count=stride*(16+(horizontal?0:radius*2));
    int4 bounds=blur_bounds(config);
    for(uint i=local.y*16+local.x;i<count;i+=256) {
        int2 position=int2(origin+uint2(i%stride,i/stride))-(horizontal?int2(radius,0):int2(0,radius));
        pixels[i]=blur_inside(position,bounds)?pack_pixel(source.read(uint2(position))):0;
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if(!output) return;
    uint center=(local.y+(horizontal?0:radius))*stride+local.x+(horizontal?radius:0);
    float4 accumulated=float4(byte_channels(pixels[center]));
    float sigma=max(deviation,0.0001f),variance=2.0f*sigma*sigma;
    float total=1,weight=exp(-1.0f/variance),decay=exp(-2.0f/variance);
    float ratio=blur_product(weight,decay,config.rounding_zero);
    for(uint distance=1;distance<=radius;++distance) {
        uint delta=horizontal?distance:distance*stride;
        total+=2.0f*weight;
        accumulated=fma(float4(byte_channels(pixels[center+delta])),weight,accumulated);
        accumulated=fma(float4(byte_channels(pixels[center-delta])),weight,accumulated);
        weight=blur_product(weight,ratio,config.rounding_zero);
        ratio=blur_product(ratio,decay,config.rounding_zero);
    }
    target.write(unpack_pixel(blur_pack(accumulated,total,config.rounding_zero)),xy);
}
