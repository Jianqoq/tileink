#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "sample.metal"

kernel void filter_downsample_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint factor=max(config.downsample,1u);
    uint2 lower=uint2(config.rect_x0,config.rect_y0),upper=uint2(config.rect_x1,config.rect_y1);
    uint2 begin=max(xy*factor,lower),end=min((xy+1)*factor,upper);
    uint pixel=0;
    if(all(begin<end)) {
        if(!config.downsample_filter) pixel=pack_pixel(source.read(clamp((begin+end-1)/2,lower,upper-1)));
        else {
            float4 sum=0;
            for(uint y=begin.y;y<end.y;++y) for(uint x=begin.x;x<end.x;++x)
                sum+=float4(byte_channels(pack_pixel(source.read(uint2(x,y)))));
            float count=float((end.x-begin.x)*(end.y-begin.y));
            pixel=pack_bytes(uint4(clamp(fma(sum,1.0f/count,0.5f),0.0f,255.0f)));
        }
    }
    target.write(unpack_pixel(pixel),xy);
}
kernel void filter_upsample_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint2 lower=uint2(config.rect_x0,config.rect_y0),upper=uint2(config.rect_x1,config.rect_y1);
    if(any(lower>=upper)) return;
    float2 position=clamp((float2(xy)+0.5f)/float(max(config.downsample,1u))-0.5f,float2(lower),float2(upper-1));
    float4 value=config.upsample_filter?sample_premul(source,uint2(config.width,config.height),position):source.read(uint2(rint(position)));
    target.write(unpack_pixel(pack_pixel(value)),xy);
}
