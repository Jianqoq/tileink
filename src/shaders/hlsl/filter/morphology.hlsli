#ifndef TILEINK_FILTER_MORPHOLOGY_HLSLI
#define TILEINK_FILTER_MORPHOLOGY_HLSLI
#include "config.hlsli"
#include "../shared/pixel.hlsli"
static const uint MORPHOLOGY_ERODE=0u;
static const uint MORPHOLOGY_DILATE=1u;

uint filter_morphology_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, uint2 xy) {
    uint radius=config.morphology_radius;
    bool horizontal=config.morphology_axis==0u;
    uint pos=horizontal ? xy.x : xy.y;
    uint line_len=horizontal ? config.width : config.height;
    if (config.morphology_operator==MORPHOLOGY_ERODE && (pos<radius || pos+radius>=line_len)) return 0u;
    bool dilate=config.morphology_operator==MORPHOLOGY_DILATE;
    float4 result=dilate ? 0.0 : 1.0;
    uint start=pos>radius ? pos-radius : 0u;
    uint end=min(line_len-1u,pos+radius);
    for (uint sample_pos=start; sample_pos<=end; ++sample_pos) {
        uint2 sample_xy=horizontal ? uint2(sample_pos,xy.y) : uint2(xy.x,sample_pos);
        uint pixel=unorm_to_rgba8(source.Load(int3(sample_xy,0)));
        uint alpha=pixel>>24u;
        float4 value=float4(straight_channel(pixel&255u,alpha),straight_channel((pixel>>8u)&255u,alpha),
            straight_channel((pixel>>16u)&255u,alpha),float(alpha)/255.0);
        result=dilate ? max(result,value) : min(result,value);
    }
    return pack_premul_rgba8(result.r*result.a,result.g*result.a,result.b*result.a,result.a);
}
#endif
