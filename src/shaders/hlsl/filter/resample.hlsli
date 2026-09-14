#ifndef TILEINK_FILTER_RESAMPLE_HLSLI
#define TILEINK_FILTER_RESAMPLE_HLSLI
#include "config.hlsli"
#include "sample.hlsli"
#include "../shared/pixel.hlsli"
uint filter_downsample_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, uint2 xy) {
    uint factor=max(config.downsample,1u);
    uint2 lower=uint2(config.rect_x0,config.rect_y0), upper=uint2(config.rect_x1,config.rect_y1);
    uint2 begin=max(xy*factor,lower), end=min((xy+1u)*factor,upper);
    if (any(begin>=end)) return 0u;
    if (config.downsample_filter==0u) {
        uint2 position=clamp((begin+end-1u)/2u,lower,upper-1u);
        return unorm_to_rgba8(source.Load(int3(position,0)));
    }
    float4 sum=0.0; float count=0.0;
    for (uint sy=begin.y;sy<end.y;++sy) {
        for (uint sx=begin.x;sx<end.x;++sx) {
            sum+=rgba8_to_unorm(unorm_to_rgba8(source.Load(int3(sx,sy,0))));
            count+=1.0;
        }
    }
    float4 average=sum/count;
    return pack_premul_rgba8(average.r,average.g,average.b,average.a);
}
uint filter_upsample_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, uint2 xy, uint4 source_bounds) {
    float factor=float(max(config.downsample,1u));
    float2 lower=float2(source_bounds.xy);
    float2 upper=float2(source_bounds.zw-1u);
    float2 position=clamp((float2(xy)+0.5)/factor-0.5,lower,upper);
    if (config.upsample_filter==0u) return unorm_to_rgba8(source.Load(int3(int2(round(position)),0)));
    float4 pixel=filter_sample_premul(source,uint2(config.width,config.height),position);
    return pack_premul_rgba8(pixel.r,pixel.g,pixel.b,pixel.a);
}
#endif
