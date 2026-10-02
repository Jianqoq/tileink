#ifndef TILEINK_HLSL_BRUSH_SWEEP_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_SWEEP_HLSLI_INCLUDED
#include "data.hlsli"
#include "ramp.hlsli"
static const float GRADIENT_TAU=6.2831855;
uint sample_sweep(ByteAddressBuffer paint,uint brush_base,float x,float y,uint base,uint extend,uint payload_offset,uint payload_len) {
    float cx=brush_param(paint,brush_base,base,0u),cy=brush_param(paint,brush_base,base,1u);
    float start_angle=brush_param(paint,brush_base,base,2u),end_angle=brush_param(paint,brush_base,base,3u);
    float span=end_angle-start_angle,t=0.0;
    if (abs(span)>GRADIENT_FLOAT_EPSILON) {
        // The sweep center has no direction; define zero before atan2(0,0).
        float angle=0.0;
        if(x!=cx || y!=cy) angle=atan2(y-cy,x-cx);
        if(span>0.0) {while(angle<start_angle) angle+=GRADIENT_TAU;}
        else {while(angle>start_angle) angle-=GRADIENT_TAU;}
        t=(angle-start_angle)/span;
    }
    return sample_ramp(paint,brush_base,payload_offset,payload_len,t,extend);
}
#endif // TILEINK_HLSL_BRUSH_SWEEP_HLSLI_INCLUDED
