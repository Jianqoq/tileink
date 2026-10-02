#ifndef TILEINK_HLSL_BRUSH_LINEAR_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_LINEAR_HLSLI_INCLUDED
#include "data.hlsli"
#include "ramp.hlsli"
uint sample_linear(ByteAddressBuffer paint,uint brush_base,float x,float y,uint base,uint extend,uint payload_offset,uint payload_len) {
    float tx=mad(brush_param(paint,brush_base,base,4u),x,mad(brush_param(paint,brush_base,base,6u),y,brush_param(paint,brush_base,base,8u)));
    float ty=mad(brush_param(paint,brush_base,base,5u),x,mad(brush_param(paint,brush_base,base,7u),y,brush_param(paint,brush_base,base,9u)));
    float sx=brush_param(paint,brush_base,base,0u),sy=brush_param(paint,brush_base,base,1u);
    float ex=brush_param(paint,brush_base,base,2u),ey=brush_param(paint,brush_base,base,3u);
    float dx=ex-sx,dy=ey-sy;
    float denominator=dx*dx+dy*dy, t=0.0;
    if (denominator>GRADIENT_FLOAT_EPSILON) t=mad(tx-sx,dx,(ty-sy)*dy)/denominator;
    return sample_ramp(paint,brush_base,payload_offset,payload_len,t,extend);
}
#endif // TILEINK_HLSL_BRUSH_LINEAR_HLSLI_INCLUDED
