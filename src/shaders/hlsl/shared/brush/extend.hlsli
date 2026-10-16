#ifndef TILEINK_HLSL_BRUSH_EXTEND_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_EXTEND_HLSLI_INCLUDED
#include "constants.hlsli"
#include "../pixel.hlsli"
float apply_extend(float t,uint extend) {
    if (extend==BRUSH_EXTEND_REPEAT) return rem_euclid_f32(t,1.0);
    if (extend==BRUSH_EXTEND_REFLECT) {
        float value=rem_euclid_f32(t,2.0);
        return value<=1.0?value:2.0-value;
    }
    return clamp(t,0.0,1.0);
}
#endif // TILEINK_HLSL_BRUSH_EXTEND_HLSLI_INCLUDED
