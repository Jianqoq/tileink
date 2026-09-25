#ifndef TILEINK_FILTER_COMPOSITE_HLSLI
#define TILEINK_FILTER_COMPOSITE_HLSLI
#include "../shared/blend.hlsli"
#include "../shared/pixel.hlsli"

static const uint SVG_COMPOSITE_OVER=0u;
static const uint SVG_COMPOSITE_IN=1u;
static const uint SVG_COMPOSITE_OUT=2u;
static const uint SVG_COMPOSITE_ATOP=3u;
static const uint SVG_COMPOSITE_XOR=4u;
static const uint SVG_COMPOSITE_ARITHMETIC=5u;

float filter_arithmetic_channel(float a, float b, float4 coefficients) {
    return clamp(coefficients.x*a*b + coefficients.y*a + coefficients.z*b + coefficients.w,0.0,1.0);
}
uint filter_composite_pixel(uint source, uint backdrop, uint operation, float4 coefficients) {
    if (operation==SVG_COMPOSITE_ARITHMETIC) {
        float4 a=rgba8_to_unorm(source), b=rgba8_to_unorm(backdrop);
        return pack_premul_rgba8(filter_arithmetic_channel(a.r,b.r,coefficients),
            filter_arithmetic_channel(a.g,b.g,coefficients),
            filter_arithmetic_channel(a.b,b.b,coefficients),
            filter_arithmetic_channel(a.a,b.a,coefficients));
    }
    uint compose=COMPOSE_SRC_OVER;
    if (operation==SVG_COMPOSITE_IN) compose=COMPOSE_SRC_IN;
    else if (operation==SVG_COMPOSITE_OUT) compose=COMPOSE_SRC_OUT;
    else if (operation==SVG_COMPOSITE_ATOP) compose=COMPOSE_SRC_ATOP;
    else if (operation==SVG_COMPOSITE_XOR) compose=COMPOSE_XOR;
    return blend_premul_u8(backdrop,source,compose<<8u);
}
#endif
