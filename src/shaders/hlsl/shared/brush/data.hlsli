#ifndef TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
uint brush_word(ByteAddressBuffer paint,uint brush_base,uint index) {
    uint bytes;paint.GetDimensions(bytes);
    uint count=bytes/4u;
    // Preserve WGSL robust reads without allowing address addition to wrap.
    // Keep the load inside both valid-range branches. The equivalent early-out
    // OR guard loses valid pattern reads on the tested AMD Vulkan compilation
    // path (driver 24.30.18); this workaround retains identical bounds semantics.
    if (brush_base<count) {
        if (index<count-brush_base) return paint.Load((brush_base+index)*4u);
    }
    return 0u;
}
float brush_param(ByteAddressBuffer paint,uint brush_base,uint base,uint index) {
    return asfloat(brush_word(paint,brush_base,base+index));
}
#endif // TILEINK_HLSL_BRUSH_DATA_HLSLI_INCLUDED
