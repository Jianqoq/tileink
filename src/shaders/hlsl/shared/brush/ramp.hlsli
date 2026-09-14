#ifndef TILEINK_HLSL_BRUSH_RAMP_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_RAMP_HLSLI_INCLUDED
#include "data.hlsli"
#include "extend.hlsli"
#include "../pixel.hlsli"
static const float GRADIENT_FLOAT_EPSILON=0.00000011920929;
uint sample_ramp(ByteAddressBuffer paint,uint brush_base,uint payload_offset,uint payload_len,float t,uint extend) {
    if (payload_len==0u) return 0u;
    uint last=payload_len-1u;
    float position=apply_extend(t,extend)*float(last);
    uint left_ix=uint(floor(position)), right_ix=min(left_ix+1u,last);
    float fraction=position-float(left_ix);
    uint left=brush_word(paint,brush_base,payload_offset+left_ix);
    uint right=brush_word(paint,brush_base,payload_offset+right_ix);
    if (fraction<=GRADIENT_FLOAT_EPSILON || left_ix==right_ix) return left;
    return lerp_premul_u8(left,right,fraction);
}
#endif // TILEINK_HLSL_BRUSH_RAMP_HLSLI_INCLUDED
