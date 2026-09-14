#ifndef TILEINK_FILTER_COLOR_SPACE_HLSLI
#define TILEINK_FILTER_COLOR_SPACE_HLSLI
float filter_srgb_to_linear(float value) {
    return value>0.04045 ? pow((value+0.055)/1.055,2.4) : value/12.92;
}
#endif
