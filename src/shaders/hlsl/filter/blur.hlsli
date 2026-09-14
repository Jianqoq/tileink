#ifndef TILEINK_FILTER_BLUR_HLSLI
#define TILEINK_FILTER_BLUR_HLSLI
#include "config.hlsli"
#include "sample.hlsli"
#include "../shared/pixel.hlsli"

float4 blur_channels(uint pixel) {
    return float4(pixel&255u,(pixel>>8u)&255u,(pixel>>16u)&255u,pixel>>24u);
}
uint blur_pack_average(float4 accumulator,float sum) {
    if (sum<=0.0) return 0u;
    float4 average=mad(accumulator,1.0/sum,0.0);
    uint4 bytes=uint4(clamp(average+0.5,0.0,255.0));
    return rgba8_pack(bytes.r,bytes.g,bytes.b,bytes.a);
}
int blur_half_width(float std_dev) { return int(max(ceil(std_dev*3.0),1.0)); }
int4 blur_sample_bounds(ConstantBuffer<FilterConfig> config) {
    bool source_x=config.source_x1>config.source_x0;
    bool source_y=config.source_y1>config.source_y0;
    return int4(source_x ? config.source_x0 : config.region_x0,source_y ? config.source_y0 : config.region_y0,
        source_x ? config.source_x1 : config.region_x0+config.region_width,
        source_y ? config.source_y1 : config.region_y0+config.region_height);
}
bool blur_inside(int2 position,int4 bounds) {
    return all(position>=bounds.xy) && all(position<bounds.zw);
}
bool blur_pair_inside(int2 xy,int2 axis,float offset,int4 bounds) {
    float2 first=float2(xy)+float2(axis)*offset;
    float2 next=first+float2(axis);
    return all(first>=float2(bounds.xy)) && all(next<float2(bounds.zw));
}
float4 blur_sample_pair(ConstantBuffer<FilterConfig> config,Texture2D<float4> source,int2 xy,int2 axis,float offset) {
    float4 sample=filter_sample_premul(source,uint2(config.width,config.height),float2(xy)+float2(axis)*offset);
    return mad(sample,255.0,0.0);
}
uint filter_blur_pixel(ConstantBuffer<FilterConfig> config,Texture2D<float4> source,uint2 xy,float std_dev) {
    int half_width=blur_half_width(std_dev);
    float sigma=max(std_dev,0.0001);
    float two_sigma_sq=2.0*sigma*sigma;
    int4 bounds=blur_sample_bounds(config);
    int2 axis=config.blur_axis==0u ? int2(1,0) : int2(0,1);
    int2 base=int2(xy);
    bool interior=blur_inside(base-axis*half_width,bounds) && blur_inside(base+axis*half_width,bounds);
    float sum=1.0;
    // The center is a tap too: data outside the valid source domain is transparent.
    uint center=0u;
    if (blur_inside(base,bounds)) center=unorm_to_rgba8(source.Load(int3(xy,0)));
    float4 accumulator=blur_channels(center);
    float weight=exp(-1.0/two_sigma_sq);
    float decay=exp(-2.0/two_sigma_sq);
    float ratio=weight*decay;
    for (int d=1;d<=half_width;d+=2) {
        int next_d=d+1;
        bool pair=next_d<=half_width;
        float next_weight=weight*ratio;
        float pair_weight=weight+(pair ? next_weight : 0.0);
        sum+=2.0*pair_weight;
        if (pair && (interior || blur_pair_inside(base,axis,float(d),bounds))) {
            float offset=float(d)+next_weight/pair_weight;
            accumulator=mad(blur_sample_pair(config,source,base,axis,offset),pair_weight,accumulator);
        } else {
            int2 position=base+axis*d;
            if (interior || blur_inside(position,bounds))
                accumulator=mad(blur_channels(unorm_to_rgba8(source.Load(int3(position,0)))),weight,accumulator);
            if (pair) {
                position=base+axis*next_d;
                if (interior || blur_inside(position,bounds))
                    accumulator=mad(blur_channels(unorm_to_rgba8(source.Load(int3(position,0)))),next_weight,accumulator);
            }
        }
        if (pair && (interior || blur_pair_inside(base,axis,-float(next_d),bounds))) {
            float offset=-(float(d)+next_weight/pair_weight);
            accumulator=mad(blur_sample_pair(config,source,base,axis,offset),pair_weight,accumulator);
        } else {
            int2 position=base-axis*d;
            if (interior || blur_inside(position,bounds))
                accumulator=mad(blur_channels(unorm_to_rgba8(source.Load(int3(position,0)))),weight,accumulator);
            if (pair) {
                position=base-axis*next_d;
                if (interior || blur_inside(position,bounds))
                    accumulator=mad(blur_channels(unorm_to_rgba8(source.Load(int3(position,0)))),next_weight,accumulator);
            }
        }
        // Each recurrence product has the same explicit rounding boundary as WGSL.
        weight=mad(next_weight,mad(ratio,decay,0.0),0.0);
        ratio=mad(ratio,mad(decay,decay,0.0),0.0);
    }
    return blur_pack_average(accumulator,sum);
}
#endif
