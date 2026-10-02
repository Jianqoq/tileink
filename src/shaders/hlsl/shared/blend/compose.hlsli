#ifndef TILEINK_HLSL_BLEND_COMPOSE_HLSLI_INCLUDED
#define TILEINK_HLSL_BLEND_COMPOSE_HLSLI_INCLUDED
#include "modes.hlsli"
float compose_src_factor(uint compose,float dst_alpha) {
    if (compose==COMPOSE_CLEAR || compose==COMPOSE_DEST || compose==COMPOSE_DEST_IN || compose==COMPOSE_DEST_OUT) return 0.0;
    if (compose==COMPOSE_DEST_OVER || compose==COMPOSE_SRC_OUT || compose==COMPOSE_DEST_ATOP || compose==COMPOSE_XOR) return 1.0-dst_alpha;
    if (compose==COMPOSE_SRC_IN || compose==COMPOSE_SRC_ATOP) return dst_alpha;
    return 1.0;
}
float compose_dst_factor(uint compose,float src_alpha) {
    if (compose==COMPOSE_CLEAR || compose==COMPOSE_COPY || compose==COMPOSE_SRC_IN || compose==COMPOSE_SRC_OUT) return 0.0;
    if (compose==COMPOSE_DEST || compose==COMPOSE_DEST_OVER || compose==COMPOSE_PLUS || compose==COMPOSE_PLUS_LIGHTER) return 1.0;
    if (compose==COMPOSE_DEST_IN || compose==COMPOSE_DEST_ATOP) return src_alpha;
    return 1.0-src_alpha;
}
float color_dodge_premul(float src,float dst,float src_alpha,float dst_alpha) {
    float result=src*(1.0-dst_alpha);
    if (dst>0.0) {
        if (src>=src_alpha) result=src+dst*(1.0-src_alpha);
        else result=src_alpha*min(dst_alpha,(dst*src_alpha)/(src_alpha-src))+src*(1.0-dst_alpha)+dst*(1.0-src_alpha);
    }
    return result;
}
float color_burn_premul(float src,float dst,float src_alpha,float dst_alpha) {
    float result=dst+src*(1.0-dst_alpha);
    if (dst<dst_alpha) {
        if (src<=0.0) result=dst*(1.0-src_alpha);
        else result=src_alpha*(dst_alpha-min(dst_alpha,((dst_alpha-dst)*src_alpha)/src))+src*(1.0-dst_alpha)+dst*(1.0-src_alpha);
    }
    return result;
}
#endif // TILEINK_HLSL_BLEND_COMPOSE_HLSLI_INCLUDED
