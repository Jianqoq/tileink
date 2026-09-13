#ifndef TILEINK_HLSL_BLEND_SEPARABLE_HLSLI_INCLUDED
#define TILEINK_HLSL_BLEND_SEPARABLE_HLSLI_INCLUDED
#include "modes.hlsli"
float overlay(float dst,float src) {
    if (dst<=0.5) return 2.0*dst*src;
    return 1.0-2.0*(1.0-dst)*(1.0-src);
}
float color_dodge(float dst,float src) {
    // Backdrop endpoints precede the source singularity (W3C compositing).
    if (dst==0.0) return 0.0;
    if (src<1.0) return min(dst/(1.0-src),1.0);
    return 1.0;
}
float color_burn(float dst,float src) {
    if (dst==1.0) return 1.0;
    if (src>0.0) return 1.0-min((1.0-dst)/src,1.0);
    return 0.0;
}
float soft_light(float dst,float src) {
    float result=dst-(1.0-2.0*src)*dst*(1.0-dst);
    if (src>0.5) {
        float d=sqrt(dst);
        if (dst<=0.25) d=((16.0*dst-12.0)*dst+4.0)*dst;
        result=dst+(2.0*src-1.0)*(d-dst);
    }
    return result;
}
float mix_separable(float dst,float src,uint mode) {
    if (mode==MIX_MULTIPLY) return dst*src;
    if (mode==MIX_SCREEN) return dst+src-dst*src;
    if (mode==MIX_OVERLAY) return overlay(dst,src);
    if (mode==MIX_DARKEN) return min(dst,src);
    if (mode==MIX_LIGHTEN) return max(dst,src);
    if (mode==MIX_COLOR_DODGE) return color_dodge(dst,src);
    if (mode==MIX_COLOR_BURN) return color_burn(dst,src);
    if (mode==MIX_HARD_LIGHT) return overlay(src,dst);
    if (mode==MIX_SOFT_LIGHT) return soft_light(dst,src);
    if (mode==MIX_DIFFERENCE) return abs(dst-src);
    if (mode==MIX_EXCLUSION) return dst+src-2.0*dst*src;
    return src;
}
#endif // TILEINK_HLSL_BLEND_SEPARABLE_HLSLI_INCLUDED
