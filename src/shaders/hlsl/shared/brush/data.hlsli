#ifndef TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
uint brush_word(ByteAddressBuffer paint,uint brush_base,uint index) {
    uint bytes;paint.GetDimensions(bytes);
    uint count=bytes/4u;
    // Preserve WGSL robust reads without allowing address addition to wrap.
    if (brush_base>=count || index>=count-brush_base) return 0u;
    return paint.Load((brush_base+index)*4u);
}
float brush_param(ByteAddressBuffer paint,uint brush_base,uint base,uint index) {
    return asfloat(brush_word(paint,brush_base,base+index));
}
#endif // TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
