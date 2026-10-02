#ifndef TILEINK_HLSL_SDF_RECT_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_RECT_HLSLI_INCLUDED
#include "basic.hlsli"
float rect_sdf_distance(float2 position,float4 bounds,float4 radii) {
    return rect_sdf_distance(position.x,position.y,bounds.x,bounds.y,bounds.z,bounds.w,radii.x,radii.y,radii.z,radii.w);
}
#endif
