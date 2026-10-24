#ifndef TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
// Recover the second product's rounding error before translation so a rotated
// pattern's exact zero cannot become a negative nearest-sampling coordinate.
// A local precise qualifier can propagate into other production FMad operations
// and make them non-fused; the full-renderer regressions cover that interaction.
float pattern_transform_component(float a,float b,float offset,float x,float y) {
    float by=b*y;
    float dot_product=mad(a,x,by)+mad(b,y,-by);
    return dot_product+offset;
}
#endif // TILEINK_HLSL_SHARED_PATTERN_TRANSFORM_HLSLI_INCLUDED
