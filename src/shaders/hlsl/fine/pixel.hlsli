#ifndef TILEINK_HLSL_FINE_PIXEL_INCLUDED
#define TILEINK_HLSL_FINE_PIXEL_INCLUDED
#include "inputs.hlsli"
#include "../shared/pixel.hlsli"

bool premul_u8_is_opaque(uint pixel) { return (pixel>>24u)==255u; }
float4 fine_initial_pixel(FineInputs input,uint x,uint y) {
    return input.config.load_target!=0u ? input.target[uint2(x,y)] : rgba8_to_unorm(input.config.clear_color);
}

#endif
