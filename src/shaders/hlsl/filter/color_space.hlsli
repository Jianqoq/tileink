#ifndef TILEINK_FILTER_COLOR_SPACE_HLSLI
#define TILEINK_FILTER_COLOR_SPACE_HLSLI
float filter_srgb_to_linear(float value) {
    return value>0.04045 ? pow((value+0.055)/1.055,2.4) : value/12.92;
}
float filter_linear_rgb_to_srgb(float value) {
    return value>0.0031308 ? 1.055*pow(value,1.0/2.4)-0.055 : value*12.92;
}
#endif
