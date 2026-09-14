#ifndef TILEINK_HLSL_SDF_TYPES_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_TYPES_HLSLI_INCLUDED
struct AffineRecord { float a,b,c,d,e,f; };
struct SdfSample { float distance; float2 normal; };
SdfSample make_sdf_sample(float distance,float2 normal) { SdfSample result; result.distance=distance; result.normal=normal; return result; }
#endif // TILEINK_HLSL_SDF_TYPES_HLSLI_INCLUDED
