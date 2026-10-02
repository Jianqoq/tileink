#ifndef TILEINK_FILTER_COLOR_SPACE_METAL
#define TILEINK_FILTER_COLOR_SPACE_METAL
float filter_srgb_to_linear(float value) { return value>0.04045f?pow((value+0.055f)/1.055f,2.4f):value/12.92f; }
float filter_linear_to_srgb(float value) { return value>0.0031308f?1.055f*pow(value,1.0f/2.4f)-0.055f:value*12.92f; }
float3 filter_srgb_to_linear_rgb(float3 value) {
    return float3(filter_srgb_to_linear(value.r),filter_srgb_to_linear(value.g),filter_srgb_to_linear(value.b));
}
float3 filter_linear_to_srgb_rgb(float3 value) {
    return float3(filter_linear_to_srgb(value.r),filter_linear_to_srgb(value.g),filter_linear_to_srgb(value.b));
}
uint filter_premul_srgb_to_linear(uint pixel) {
    float4 value=unpack_pixel(pixel);
    if(value.a==0.0f) return 0;
    float3 rgb=filter_srgb_to_linear_rgb(clamp(value.rgb/value.a,0.0f,1.0f))*value.a;
    return pack_pixel(float4(rgb,value.a));
}
uint filter_premul_linear_to_srgb(uint pixel) {
    float4 value=unpack_pixel(pixel);
    if(value.a==0.0f) return 0;
    float3 rgb=filter_linear_to_srgb_rgb(clamp(value.rgb/value.a,0.0f,1.0f))*value.a;
    return pack_pixel(float4(rgb,value.a));
}
#endif
