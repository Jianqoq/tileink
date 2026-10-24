#ifndef TILEINK_HLSL_BRUSH_FOUR_CORNER_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_FOUR_CORNER_HLSLI_INCLUDED
#include "data.hlsli"
#include "ramp.hlsli"
#include "../pixel.hlsli"
uint sample_four_corner(ByteAddressBuffer paint,uint brush_base,float x,float y,uint base,uint payload_offset) {
    float x0=brush_param(paint,brush_base,base,0u),y0=brush_param(paint,brush_base,base,1u);
    float x1=brush_param(paint,brush_base,base,2u),y1=brush_param(paint,brush_base,base,3u);
    float width=x1-x0,height=y1-y0,u=0.0,v=0.0;
    if(abs(width)>GRADIENT_FLOAT_EPSILON) u=clamp((x-x0)/width,0.0,1.0);
    if(abs(height)>GRADIENT_FLOAT_EPSILON) v=clamp((y-y0)/height,0.0,1.0);
    uint tl=brush_word(paint,brush_base,payload_offset),tr=brush_word(paint,brush_base,payload_offset+1u);
    uint br=brush_word(paint,brush_base,payload_offset+2u),bl=brush_word(paint,brush_base,payload_offset+3u);
    return lerp_premul_u8(lerp_premul_u8(tl,tr,u),lerp_premul_u8(bl,br,u),v);
}
#endif // TILEINK_HLSL_BRUSH_FOUR_CORNER_HLSLI_INCLUDED
