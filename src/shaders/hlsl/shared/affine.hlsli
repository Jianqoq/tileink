#ifndef TILEINK_HLSL_SHARED_AFFINE_INCLUDED
#define TILEINK_HLSL_SHARED_AFFINE_INCLUDED
#include "../scene_records.hlsli"
struct AffineRecord { float a,b,c,d,e,f; };
AffineRecord load_affine(ByteAddressBuffer records, uint address) {
    float4 coefficients=asfloat(records.Load4(address));
    float2 translation=asfloat(records.Load2(address+AFFINE_TRANSLATION));
    AffineRecord result;
    result.a=coefficients.x; result.b=coefficients.y; result.c=coefficients.z; result.d=coefficients.w;
    result.e=translation.x; result.f=translation.y;
    return result;
}
// Fix affine evaluation order before the half-alpha coverage boundary.
float2 affine_record_point(AffineRecord transform, float2 sample_point) {
    return float2(
        mad(transform.a,sample_point.x,mad(transform.c,sample_point.y,transform.e)),
        mad(transform.b,sample_point.x,mad(transform.d,sample_point.y,transform.f)));
}

#endif
