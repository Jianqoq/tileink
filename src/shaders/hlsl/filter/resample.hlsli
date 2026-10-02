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
    // Accumulate stored bytes, avoiding normalization error before half-byte rounding.
    float4 sum=0.0;
    for (uint sy=begin.y;sy<end.y;++sy) {
        for (uint sx=begin.x;sx<end.x;++sx) {
            uint pixel=unorm_to_rgba8(source.Load(int3(sx,sy,0)));
            sum+=float4(pixel&255u,(pixel>>8u)&255u,(pixel>>16u)&255u,pixel>>24u);
        }
    }
    float count=float((end.x-begin.x)*(end.y-begin.y));
    uint4 average=uint4(clamp(mad(sum,1.0/count,0.5),0.0,255.0));
    return rgba8_pack(average.r,average.g,average.b,average.a);
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
