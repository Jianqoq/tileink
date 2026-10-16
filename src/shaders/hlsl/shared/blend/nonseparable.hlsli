#ifndef TILEINK_HLSL_BLEND_NONSEPARABLE_HLSLI_INCLUDED
#define TILEINK_HLSL_BLEND_NONSEPARABLE_HLSLI_INCLUDED
// Match the explicit WGSL luminosity FMA order at half-channel boundaries.
float lum3(float3 rgb) { return mad(0.3,rgb.r,mad(0.59,rgb.g,0.11*rgb.b)); }
float sat3(float3 rgb) { return max(max(rgb.r,rgb.g),rgb.b)-min(min(rgb.r,rgb.g),rgb.b); }
float3 clip_color(float3 rgb) {
    float lum=lum3(rgb);
    float min_c=min(min(rgb.r,rgb.g),rgb.b), max_c=max(max(rgb.r,rgb.g),rgb.b);
    float3 result=rgb;
    if (min_c<0.0) result=lum+(result-lum)*lum/(lum-min_c);
    if (max_c>1.0) result=lum+(result-lum)*(1.0-lum)/(max_c-lum);
    return result;
}
float3 set_lum(float3 rgb,float lum) {
    float d=lum-lum3(rgb);
    return clip_color(rgb+d);
}
float3 set_sat(float3 rgb,float sat) {
    uint min_ix=0u;
    if (rgb.r<=rgb.g && rgb.r<=rgb.b) {} else if (rgb.g<=rgb.b) min_ix=1u; else min_ix=2u;
    uint max_ix=0u;
    if (rgb.r>=rgb.g && rgb.r>=rgb.b) {} else if (rgb.g>=rgb.b) max_ix=1u; else max_ix=2u;
    float3 result=float3(0.0,0.0,0.0);
    if (min_ix!=max_ix) {
        uint mid_ix=3u-min_ix-max_ix;
        float minimum=rgb[min_ix], middle=rgb[mid_ix], maximum=rgb[max_ix];
        if (maximum>minimum) {
            result[mid_ix]=(middle-minimum)*sat/(maximum-minimum);
            result[max_ix]=sat;
        }
    }
    return result;
}
#endif // TILEINK_HLSL_BLEND_NONSEPARABLE_HLSLI_INCLUDED
