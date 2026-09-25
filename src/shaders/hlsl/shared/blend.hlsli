#ifndef TILEINK_HLSL_SHARED_BLEND_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_BLEND_HLSLI_INCLUDED
#include "pixel.hlsli"
#include "blend/modes.hlsli"
#include "blend/separable.hlsli"
#include "blend/nonseparable.hlsli"
#include "blend/compose.hlsli"

float3 mix_rgb(float3 dst,float3 src,uint mode) {
    if (mode==MIX_HUE) return set_lum(set_sat(src,sat3(dst)),lum3(dst));
    if (mode==MIX_SATURATION) return set_lum(set_sat(dst,sat3(src)),lum3(dst));
    if (mode==MIX_COLOR) return set_lum(src,lum3(dst));
    if (mode==MIX_LUMINOSITY) return set_lum(dst,lum3(src));
    return float3(mix_separable(dst.r,src.r,mode),mix_separable(dst.g,src.g,mode),mix_separable(dst.b,src.b,mode));
}
uint blend_premul_u8(uint destination,uint source,uint mode) {
    uint mix=mode&CHANNEL_MAX, compose=(mode>>8u)&CHANNEL_MAX;
    float4 src=rgba8_to_unorm(source), dst=rgba8_to_unorm(destination);
    float4 result=src+dst*(1.0-src.a);
    if (mix==MIX_NORMAL) {
        if (compose==COMPOSE_DEST) result=dst;
        else if (compose==COMPOSE_CLEAR) result=float4(0.0,0.0,0.0,0.0);
        else if (compose==COMPOSE_COPY) result=src;
        else if (compose!=COMPOSE_SRC_OVER) {
            result=src*compose_src_factor(compose,dst.a)+dst*compose_dst_factor(compose,src.a);
            if (compose==COMPOSE_PLUS_LIGHTER) result=min(result,1.0);
        }
    } else if (compose==COMPOSE_SRC_OVER && mix==MIX_COLOR_DODGE) {
        result.rgb=float3(color_dodge_premul(src.r,dst.r,src.a,dst.a),color_dodge_premul(src.g,dst.g,src.a,dst.a),color_dodge_premul(src.b,dst.b,src.a,dst.a));
    } else if (compose==COMPOSE_SRC_OVER && mix==MIX_COLOR_BURN) {
        result.rgb=float3(color_burn_premul(src.r,dst.r,src.a,dst.a),color_burn_premul(src.g,dst.g,src.a,dst.a),color_burn_premul(src.b,dst.b,src.a,dst.a));
    } else {
        float src_alpha=clamp(src.a,0.0,1.0), dst_alpha=clamp(dst.a,0.0,1.0);
        float3 straight_src=float3(0.0,0.0,0.0), straight_dst=float3(0.0,0.0,0.0);
        if (src_alpha>0.0) straight_src=src.rgb/src_alpha;
        if (dst_alpha>0.0) straight_dst=dst.rgb/dst_alpha;
        float3 mixed=mix_rgb(straight_dst,straight_src,mix);
        // Explicit fusion keeps scalar/vector backends on the same half-channel boundary.
        float3 effective=src_alpha*mad(dst_alpha,mixed,(1.0-dst_alpha)*straight_src);
        float src_factor=compose_src_factor(compose,dst_alpha), dst_factor=compose_dst_factor(compose,src_alpha);
        result.rgb=effective*src_factor+dst.rgb*dst_factor;
        result.a=src_alpha*src_factor+dst_alpha*dst_factor;
    }
    return pack_premul_rgba8(result.r,result.g,result.b,result.a);
}
#endif // TILEINK_HLSL_SHARED_BLEND_HLSLI_INCLUDED
