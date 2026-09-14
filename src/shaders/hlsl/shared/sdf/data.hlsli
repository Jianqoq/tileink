#ifndef TILEINK_HLSL_SDF_DATA_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_DATA_HLSLI_INCLUDED
uint sdf_word(ByteAddressBuffer paint,uint base,uint index) {
    uint bytes;paint.GetDimensions(bytes);
    uint count=bytes/4u;
    if(base>=count || index>=count-base) return 0u;
    return paint.Load((base+index)*4u);
}
float sdf_float(ByteAddressBuffer paint,uint base,uint index) { return asfloat(sdf_word(paint,base,index)); }
#endif // TILEINK_HLSL_SDF_DATA_HLSLI_INCLUDED
