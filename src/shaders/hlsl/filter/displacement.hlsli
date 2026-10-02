#ifndef TILEINK_FILTER_DISPLACEMENT_HLSLI
#define TILEINK_FILTER_DISPLACEMENT_HLSLI
#include "../shared/pixel.hlsli"
#include "color_space.hlsli"
float filter_displacement_channel(uint pixel,uint channel,bool linear_rgb) {
    uint alpha=pixel>>24u;
    if (channel==3u) return float(alpha)/255.0;
    uint value=(pixel>>(channel*8u))&255u;
    float straight=straight_channel(value,alpha);
    return linear_rgb ? filter_srgb_to_linear(straight) : straight;
}
#endif
