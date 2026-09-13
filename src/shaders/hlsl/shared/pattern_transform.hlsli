#ifndef TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
// Recover the second product's rounding error before translation so a rotated
// pattern's exact zero cannot become a negative nearest-sampling coordinate.
float pattern_transform_component(float a,float b,float offset,float x,float y) {
    precise float by=b*y;
    precise float dot_product=mad(a,x,by)+mad(b,y,-by);
    return dot_product+offset;
}
#endif // TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
